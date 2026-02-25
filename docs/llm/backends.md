# Backends — LLM Backend Management & Routing

> SSOT | **Last Updated**: 2026-02-26

## Entities

### LlmBackend

```rust
pub struct LlmBackend {
    pub id: Uuid,
    pub name: String,
    pub backend_type: BackendType,         // Ollama | Gemini
    pub url: String,                       // Ollama: "http://host:11434", Gemini: ""
    pub api_key_encrypted: Option<String>, // Gemini API key
    pub is_active: bool,
    pub total_vram_mb: i64,               // 수기 입력, 0 = 미등록/무제한
    pub gpu_index: Option<i16>,           // 수기 입력 (0-based)
    pub server_id: Option<Uuid>,          // FK → gpu_servers (Gemini = NULL)
    pub agent_url: Option<String>,        // Phase 2 sidecar (현재 미사용)
    pub is_free_tier: bool,               // Gemini 무료 계정 여부 (RPM/RPD 카운팅 여부 결정)
    pub status: LlmBackendStatus,         // Online | Offline | Degraded
    pub registered_at: DateTime<Utc>,
}
```

> `rpm_limit`/`rpd_limit`은 백엔드 단위에서 **제거됨** — 모델 단위 공유 정책(`gemini_rate_limit_policies`)으로 대체.

### GeminiRateLimitPolicy

모델 이름 단위로 하나의 정책을 정의. 모든 무료 백엔드가 공유.

```rust
pub struct GeminiRateLimitPolicy {
    pub id: Uuid,
    pub model_name: String,           // e.g. "gemini-2.5-flash" | "*" (global default)
    pub rpm_limit: i32,               // 0 = 미적용
    pub rpd_limit: i32,               // 0 = 미적용
    pub available_on_free_tier: bool, // false → 무료 백엔드 스킵, 유료 직행
    pub updated_at: DateTime<Utc>,
}
```

`model_name = "*"` 행: 특정 모델 정책이 없을 때 사용하는 전역 기본값.

## DB Schema

```sql
-- migrations 순서 요약
-- 000005: agent_url
-- 000006: gpu_index
-- 000007: total_vram_mb
-- 000008: node_exporter_url
-- 000010: server_id UUID FK → gpu_servers
-- 000016: is_free_tier (rpm_limit, rpd_limit 임시 추가 후 제거)
-- 000017: gemini_rate_limit_policies 테이블 신규
-- 000018: llm_backends에서 rpm_limit, rpd_limit 제거
-- 000019: gemini_rate_limit_policies.available_on_free_tier 추가

-- llm_backends 최종 주요 컬럼
CREATE TABLE llm_backends (
    id                UUID         PRIMARY KEY,
    name              VARCHAR(255) NOT NULL,
    backend_type      VARCHAR(50)  NOT NULL,      -- 'ollama' | 'gemini'
    url               TEXT         NOT NULL DEFAULT '',
    api_key_encrypted TEXT,
    is_active         BOOLEAN      NOT NULL DEFAULT true,
    total_vram_mb     BIGINT       NOT NULL DEFAULT 0,
    gpu_index         SMALLINT,
    server_id         UUID         REFERENCES gpu_servers(id) ON DELETE SET NULL,
    agent_url         TEXT,
    is_free_tier      BOOLEAN      NOT NULL DEFAULT false,
    status            VARCHAR(20)  NOT NULL DEFAULT 'offline',
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Gemini 모델별 공유 rate limit 정책
CREATE TABLE gemini_rate_limit_policies (
    id                   UUID         PRIMARY KEY,
    model_name           VARCHAR(255) NOT NULL UNIQUE, -- "*" = global default
    rpm_limit            INTEGER      NOT NULL DEFAULT 0,
    rpd_limit            INTEGER      NOT NULL DEFAULT 0,
    available_on_free_tier BOOLEAN    NOT NULL DEFAULT true,
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

## API Endpoints

```
POST   /v1/backends                    RegisterBackendRequest → RegisterBackendResponse
GET    /v1/backends                    → Vec<BackendSummary>
PATCH  /v1/backends/{id}              UpdateBackendRequest → 200
DELETE /v1/backends/{id}              → 204
POST   /v1/backends/{id}/healthcheck  → { status }
GET    /v1/backends/{id}/models       → { models: Vec<String> }  (Valkey 1h 캐시)
POST   /v1/backends/{id}/models/sync  → { models, synced: true }

GET    /v1/gemini/policies             → Vec<GeminiPolicySummary>
PUT    /v1/gemini/policies/{model}     UpsertGeminiPolicyRequest → GeminiPolicySummary
```

### RegisterBackendRequest

```rust
pub struct RegisterBackendRequest {
    pub name: String,
    pub backend_type: BackendType,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub total_vram_mb: Option<i64>,
    pub gpu_index: Option<i16>,
    pub server_id: Option<Uuid>,
    pub is_free_tier: Option<bool>,
}
```

### UpdateBackendRequest

```rust
pub struct UpdateBackendRequest {
    pub name: String,
    pub url: Option<String>,
    pub api_key: Option<String>,          // "" | null → 기존 키 유지
    pub total_vram_mb: Option<i64>,
    pub gpu_index: Option<Option<i16>>,   // null → DB NULL (연결 해제)
    pub server_id: Option<Option<Uuid>>,  // null → DB NULL (연결 해제)
    pub is_free_tier: Option<bool>,
}
```

### UpsertGeminiPolicyRequest

```rust
pub struct UpsertGeminiPolicyRequest {
    pub rpm_limit: i32,
    pub rpd_limit: i32,
    pub available_on_free_tier: bool,     // default: true
}
```

## VRAM-Aware Routing (DynamicBackendRouter)

```
Client → submit() → Valkey RPUSH
queue_dispatcher_loop (BLPOP):
  1. list_active() → 모든 활성 백엔드
  2. Ollama: /api/ps로 현재 로드 VRAM 조회 → available = total - used
     Gemini: pick_gemini_backend() (정책 기반 라우팅)
  3. busy_backends (HashSet<Uuid>) → 현재 처리 중인 백엔드 제외
  4. 최적 후보 선택 → busy_backends.insert(id)
  5. tokio::spawn run_job() → 완료 시 busy_backends.remove(id)
```

### VRAM dispatch 규칙

```
total_vram_mb == 0  → 항상 dispatch 가능 (무제한)
total_vram_mb > 0   → available VRAM 최대인 서버 우선
```

## Background Health Checker

- 30초 주기, `start_health_checker()` 실행
- Ollama: `GET /api/tags` → Online | Offline
- Gemini: `POST /v1beta/models/gemini-2.0-flash:generateContent` with minimal prompt
- 상태 변경 시 DB `UPDATE llm_backends SET status = ?`

## Model Management

- `GET /v1/backends/{id}/models`
  - Ollama: `GET /api/tags` 직접 fetch
  - Gemini: `v1beta/models?key=KEY` fetch → `generateContent` 가능 모델만 필터링 + Valkey 캐시 (TTL 1h)
- `POST /v1/backends/{id}/models/sync`: Valkey 캐시 강제 갱신
- Web api-test 페이지: 모델 목록 staleTime 10분 (빈번한 재조회 방지)

---

## Gemini Rate Limit — 모델별 공유 정책

### 핵심 개념

Rate limit은 **Google Cloud 프로젝트** 단위. 같은 프로젝트의 키를 여러 개 등록해도 공유 풀에서 차감됨.
**롤링은 서로 다른 Google 계정(프로젝트)의 키를 각각 별도 LlmBackend로 등록해야 동작.**

Rate limit 수치는 백엔드마다 개별 설정하지 않고, `gemini_rate_limit_policies` 테이블에 **모델 단위**로 하나만 저장.
모든 `is_free_tier=true` 백엔드가 해당 정책을 공유해서 사용.

### available_on_free_tier 플래그

```
available_on_free_tier = true (기본)
  → 무료 백엔드들에서 RPM/RPD 순서대로 시도
  → 소진 시 유료 백엔드 fallback

available_on_free_tier = false
  → 무료 백엔드 완전 스킵, 유료 백엔드 직행
  → RPM/RPD 카운터 미증가 (유료에는 한도 없음)
```

### RPM/RPD 카운터 증가 조건

```
job 완료 후:
  if backend.is_gemini && backend.is_free_tier:
      increment_gemini_counters(pool, backend_id, model_name)
```

유료 백엔드(`is_free_tier=false`) 사용 시 카운터 미증가.

### 2026 Free Tier 기본 한도

| 모델 | RPM | RPD |
|------|-----|-----|
| gemini-2.5-pro | 5 | 100 |
| gemini-2.5-flash | 10 | 250 |
| gemini-2.5-flash-lite | 15 | 1,000 |

> 한도 변경 시: admin web `/backends` → Gemini Policies → Edit. 코드 변경 불필요.

### Valkey 카운터 키 패턴

```
inferq:gemini:rpm:{backend_id}:{model}:{minute}   TTL=120s
inferq:gemini:rpd:{backend_id}:{model}:{YYYY-MM-DD}  TTL=90000s
```

### pick_gemini_backend() 순서

```
1. policy.available_on_free_tier=false → 유료 직행
2. 무료 백엔드 순회:
   - RPD 소진 → skip
   - RPM 소진, RPD OK → 다음 분까지 대기(최대 3회)
   - 둘 다 OK → 해당 백엔드 반환
3. 전체 무료 RPD 소진 → 유료 fallback
4. Valkey 없음 → fail-open (첫 무료/유료 사용)
```

### N개 계정 등록 예시

```bash
curl -X POST http://localhost:3001/v1/backends \
  -H "X-API-Key: inferq-bootstrap-admin-key" \
  -d '{"name":"gemini-acc-1","backend_type":"gemini","api_key":"AIza...1","is_free_tier":true}'

curl -X POST http://localhost:3001/v1/backends \
  -H "X-API-Key: inferq-bootstrap-admin-key" \
  -d '{"name":"gemini-acc-2","backend_type":"gemini","api_key":"AIza...2","is_free_tier":true}'

# 모델 정책 설정 (공유)
curl -X PUT http://localhost:3001/v1/gemini/policies/gemini-2.5-flash \
  -H "X-API-Key: inferq-bootstrap-admin-key" \
  -d '{"rpm_limit":10,"rpd_limit":250,"available_on_free_tier":true}'
```

---

## Web UI (admin `/backends`)

### GPU Servers 섹션

| 열 | 내용 |
|----|------|
| Name | 서버 이름 |
| node-exporter | URL (없으면 —) |
| Registered | 등록일 |
| Actions | 삭제 |

### LLM Backends 테이블

| 열 | 내용 |
|----|------|
| Backend | 이름 + 타입 배지(ollama/gemini) + URL(Ollama) |
| Assignment | Ollama: 연결 서버·GPU Index·VRAM / Gemini: [Free Tier] or [Paid] 배지 |
| Status | online/degraded/offline 배지 |
| Registered | 등록일 |
| Actions | healthcheck · sync models(Ollama만) · edit · delete |

### Gemini Policies 섹션

| 열 | 내용 |
|----|------|
| Model | 모델명 (`*` = 전역 기본) |
| RPM | 분당 요청 한도 (0 = 미적용) |
| RPD | 일당 요청 한도 (0 = 미적용) |
| Free Tier | available_on_free_tier 표시 |
| Actions | Edit |

Edit 모달:
```
[Available on Free Tier] ── Switch (primary)
  → on 시:
    RPM Limit (req/min)  |  RPD Limit (req/day)
  → off 시: RPM/RPD 입력 숨김 (유료 직행, 카운터 미증가)
```

### Register/Edit Backend 모달 — Gemini 섹션

```
[API Key *]
[Free Tier] ── Toggle
  → on 시 힌트: "Rate limits are managed per-model in Gemini Policies."
```

> RPM/RPD는 Register/Edit 모달에서 제거됨 — Gemini Policies 섹션에서 모델 단위로 관리.
