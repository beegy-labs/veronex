# Providers -- Ollama: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-03-06 (rev: automatic allocation flow)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add field to provider API request/response | `provider_handlers.rs` -- `RegisterProviderRequest` / `UpdateProviderRequest` + migration |
| Change VRAM dispatch algorithm | `infrastructure/outbound/provider_router.rs` -- `dispatch()` function |
| Change health check logic | `infrastructure/outbound/health_checker.rs` -- `check_provider()` |
| Add new model management endpoint | `provider_handlers.rs` + `router.rs` |
| Change VRAM pool logic | `infrastructure/outbound/capacity/vram_pool.rs` -- `VramPool` |
| Change thermal throttle thresholds | `infrastructure/outbound/capacity/thermal.rs` -- `ThermalThrottleMap::update()` |
| Add new LlmProvider DB column | `migrations/` new file + `domain/entities/mod.rs` + `persistence/provider_registry.rs` |
| Change provider list cache TTL | `persistence/caching_provider_registry.rs` -- `CachingProviderRegistry::new()` TTL arg in `main.rs` |
| Toggle a model on/off per Ollama provider | `PATCH /v1/providers/{id}/selected-models/{model}` -- `set_model_enabled()` in `provider_handlers.rs` |
| Change Ollama model selection defaults | `provider_handlers.rs` -- `list_selected_models()` Ollama branch -- default is `true` |
| Change streaming/context behavior | See `docs/llm/providers/ollama-impl.md` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/mod.rs` | `LlmProvider` entity |
| `crates/veronex/src/application/ports/outbound/` | `LlmProviderRegistry` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/provider_registry.rs` | `PostgresProviderRegistry` (DB adapter) |
| `crates/veronex/src/infrastructure/outbound/persistence/caching_provider_registry.rs` | `CachingProviderRegistry` (5s TTL cache decorator) |
| `crates/veronex/src/infrastructure/outbound/ollama/adapter.rs` | `OllamaAdapter` (streaming) |
| `crates/veronex/src/infrastructure/outbound/provider_router.rs` | `DynamicProviderRouter` + `queue_dispatcher_loop` |
| `crates/veronex/src/infrastructure/outbound/health_checker.rs` | 30s background health checker |
| `crates/veronex/src/infrastructure/inbound/http/provider_handlers.rs` | CRUD + model management handlers |

---

## LlmProvider Entity

```rust
// domain/entities/mod.rs
pub struct LlmProvider {
  pub id: Uuid,
  pub name: String,
  pub provider_type: ProviderType,       // Ollama | Gemini
  pub url: String,                       // "http://host:11434" (Ollama) | "" (Gemini)
  pub api_key_encrypted: Option<String>,
  pub is_active: bool,
  pub total_vram_mb: i64,               // 0 = unlimited
  pub gpu_index: Option<i16>,           // 0-based GPU index on host
  pub server_id: Option<Uuid>,          // FK -> gpu_servers (Gemini = NULL)
  pub agent_url: Option<String>,        // Phase 2 sidecar (unused)
  pub is_free_tier: bool,               // Gemini only
  pub status: LlmProviderStatus,        // Online | Offline | Degraded
  pub registered_at: DateTime<Utc>,
}
```

## DB Schema

```sql
CREATE TABLE llm_providers (
  id                UUID         PRIMARY KEY,
  name              VARCHAR(255) NOT NULL,
  provider_type     VARCHAR(50)  NOT NULL,   -- 'ollama' | 'gemini'
  url               TEXT         NOT NULL DEFAULT '',
  api_key_encrypted TEXT,
  is_active         BOOLEAN      NOT NULL DEFAULT true,
  total_vram_mb     BIGINT       NOT NULL DEFAULT 0,
  gpu_index         SMALLINT,
  server_id         UUID REFERENCES gpu_servers(id) ON DELETE SET NULL,
  agent_url         TEXT,
  is_free_tier      BOOLEAN      NOT NULL DEFAULT false,
  status            VARCHAR(20)  NOT NULL DEFAULT 'offline',
  registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
-- single init migration: 0000000001_init.sql
```

---

## API Endpoints (provider_handlers.rs)

```
POST   /v1/providers                   RegisterProviderRequest -> RegisterProviderResponse
GET    /v1/providers                   -> Vec<ProviderSummary>
PATCH  /v1/providers/{id}             UpdateProviderRequest -> 200
DELETE /v1/providers/{id}             -> 204
POST   /v1/providers/{id}/sync          -> { status, models_synced, vram_updated }
       Unified: health check + model sync + VRAM probe (Ollama only)

POST   /v1/providers/sync               -> 202 { synced_count }
       Triggers sync for all Ollama providers

GET    /v1/providers/{id}/models
       Ollama -> GET /api/tags (live)
       Gemini -> 400 "Use GET /v1/gemini/models"

GET    /v1/providers/{id}/key         -> { api_key } (decrypted, admin only)

GET    /v1/providers/{id}/selected-models
       Ollama -> per-provider list (ollama_models) merged with provider_selected_models
               default is_enabled = true for rows not yet in selection table
       Gemini -> global gemini_models merged with provider_selected_models (default false)

PATCH  /v1/providers/{id}/selected-models/{model_name}
       { is_enabled: bool } -> 204   (shared handler, same table)
```

### Global Model Pool (ollama_model_handlers.rs)

```
GET  /v1/ollama/models         -> { models: ["llama3", "mistral", ...] }  // distinct, sorted
POST /v1/ollama/models/sync    -> 202 { job_id, status: "running" }       // async, no retry
GET  /v1/ollama/sync/status    -> OllamaSyncJob (progress + per-provider results)
```

See `docs/llm/providers/ollama-models.md` for full spec.

### Request Structs

```rust
pub struct RegisterProviderRequest {
  pub name: String,
  pub provider_type: ProviderType,
  pub url: Option<String>,
  pub api_key: Option<String>,
  pub total_vram_mb: Option<i64>,
  pub gpu_index: Option<i16>,
  pub server_id: Option<Uuid>,
  pub is_free_tier: Option<bool>,
}

pub struct UpdateProviderRequest {
  pub name: String,
  pub url: Option<String>,
  pub api_key: Option<String>,          // "" or null -> keeps existing key
  pub total_vram_mb: Option<i64>,
  pub gpu_index: Option<Option<i16>>,   // Some(None) -> clears FK
  pub server_id: Option<Option<Uuid>>,  // Some(None) -> clears FK
  pub is_active: Option<bool>,
  pub is_free_tier: Option<bool>,
}
```

SQL for PATCH: `COALESCE($3, api_key_encrypted)` preserves existing key when `api_key = ""`.

---

## Provider Registry Caching

`CachingProviderRegistry` wraps `PostgresProviderRegistry` with a 5-second TTL in-memory cache. This prevents hundreds of Postgres queries/second from `queue_dispatcher_loop` calling `list_all()` on every job dequeue.

- **`list_all()`**: shared read lock fast path; write lock + DB query on miss. Double-checked locking prevents thundering herd.
- **Mutating methods** (`register`, `update_status`, `update`, `deactivate`): forward to inner + invalidate cache.
- **Read-only methods** (`list_active`, `get`): forward directly (called infrequently, no cache needed).

---

## Automatic Ollama Allocation — End-to-End Flow

Ollama 프로바이더를 등록하면 모든 것이 자동으로 동작한다: 모델 동기화, VRAM 관리, 동시성 제한, throughput 학습.
관리자는 프로바이더를 등록하고 서버를 연결하면 끝이다.

### 전체 라이프사이클

```
1. REGISTER     POST /v1/providers {name, provider_type: "ollama", url}
                → health check → status: online/offline
                → POST /v1/servers {name, node_exporter_url}
                → PATCH /v1/providers/{id} {server_id, gpu_index}

2. AUTO SYNC    Background sync loop (30s tick, 300s cooldown)
                → /api/version (health) → /api/tags (models) → /api/ps (loaded)
                → /api/show (architecture) → throughput stats → KV compute
                → AIMD update → LLM batch analysis

3. REQUEST      POST /v1/chat/completions {model: "qwen3:8b", ...}
                → provider selection → VRAM gate → concurrency gate → dispatch

4. LEARN        Completed job → throughput recorded → next sync uses for AIMD
                → 3+ samples: AIMD adjusts max_concurrent
                → 10+ samples: LLM batch recommends optimal allocation

5. RESTART      Server restart → DB에서 학습 데이터 복원 → 즉시 적용
```

### Phase 1: Provider 등록 → 자동 모델 발견

```
POST /v1/providers {name: "gpu-server", provider_type: "ollama", url: "https://ollama.example.com"}
  │
  ├── health check: GET {url}/api/version
  │   → online: status = "online", 모델 sync 가능
  │   → offline: status = "offline", sync 건너뜀
  │
  ├── model sync: GET {url}/api/tags
  │   → ollama_models 테이블에 저장 (provider별)
  │   → provider_selected_models에 기본 is_enabled=true로 등록
  │   → Valkey cache: veronex:models:{provider_id} (TTL 30s)
  │
  └── 서버 연결 (선택):
      POST /v1/servers {name, node_exporter_url}
      PATCH /v1/providers/{id} {server_id, gpu_index: 0}
      → node-exporter에서 GPU VRAM, 온도 수집 가능
```

### Phase 2: 요청 → Provider 선택 → 할당

```
POST /v1/chat/completions {model: "qwen3:8b", messages: [...]}
  │
  ├── 1. API Key 인증 → account_id, tier (free/paid) 확인
  │
  ├── 2. Valkey 큐에 등록 (티어별 우선순위)
  │     paid → veronex:queue:jobs:paid   (최우선)
  │     free → veronex:queue:jobs        (표준)
  │     test → veronex:queue:jobs:test   (최후순위)
  │
  ├── 3. queue_dispatcher_loop가 Lua priority pop으로 꺼냄
  │
  ├── 4. Provider 선택 (pick_best_provider)
  │     a. active Ollama providers 목록
  │     b. 모델 필터: ollama_models에서 해당 모델 보유한 provider만
  │     c. 모델 선택 필터: provider_selected_models에서 enabled인 것만
  │     d. VRAM 정렬: available VRAM 높은 순 (여러 서버 중 가장 여유 있는 서버)
  │     e. 티어 정렬: paid key → non-free-tier 우선, free key → free-tier 우선
  │
  ├── 5. Gate 통과 (순서대로)
  │     a. Circuit Breaker: 연속 실패 provider 스킵
  │     b. Thermal: ≥85°C Soft (active>0이면 스킵), ≥92°C Hard (완전 스킵)
  │     c. Concurrency: max_concurrent 초과 → 블록 (cold start=1)
  │     d. VRAM: vram_pool.try_reserve() → KV cache + (필요시 weight) 예약
  │
  ├── 6. Dispatch → Ollama API
  │     OllamaAdapter: POST {url}/api/chat (streaming)
  │     model 미로드 시 Ollama가 자동 로드 (weight는 VRAM에 유지)
  │
  └── 7. 완료 → 정리
        Drop(VramPermit) → KV cache 반환, active_count -= 1
        circuit_breaker.on_success/on_failure
        inference_jobs 테이블에 결과 저장
```

### Phase 3: 자동 학습 — Cold Start → AIMD → LLM Batch

```
                     ┌─────────────────────────────────────────────────┐
                     │          Sync Loop (30s tick)                   │
                     │                                                 │
  ┌──────────┐       │  ┌─────────────┐   ┌─────────┐   ┌──────────┐ │
  │ 프로바이더 │──────▶│  │ Cold Start  │──▶│  AIMD   │──▶│ LLM Batch│ │
  │ 등록      │       │  │ limit = 1   │   │ ±조정   │   │ 최적 추천 │ │
  │           │       │  │ (모든 모델)  │   │ (모델별) │   │ (전체 조합)│ │
  └──────────┘       │  └──────┬──────┘   └────┬────┘   └─────┬────┘ │
                     │         │               │              │       │
                     │    sample=0         sample≥3       sample≥10   │
                     │    baseline=0       ratio 기반     LLM 분석     │
                     │                                                 │
                     │  ┌──────────────────────────────────────────┐   │
                     │  │ DB persist: model_vram_profiles          │   │
                     │  │  max_concurrent, baseline_tps            │   │
                     │  │  → 서버 재시작 시 자동 복원               │   │
                     │  └──────────────────────────────────────────┘   │
                     └─────────────────────────────────────────────────┘
```

| Phase | 조건 | max_concurrent | 동작 |
|-------|------|---------------|------|
| **Cold Start** | 새 모델, 데이터 없음 | 1 | 모델당 1건씩만. baseline 수집 |
| **AIMD** | sample ≥ 3, baseline 있음 | 자동 조정 | ratio ≥ 0.9 → +1, < 0.7 → ×3/4 |
| **LLM Batch** | 총 sample ≥ 10 | LLM 추천 | 모든 모델 조합 + VRAM + throughput 분석 |

### Phase 4: 멀티 서버 / 멀티 모델 자동 라우팅

여러 Ollama 서버를 등록하면 자동으로 최적 서버에 라우팅된다.

```
예시: 3대 서버, 다양한 모델

Server A (128GB GPU)                    Server B (24GB GPU)          Server C (CPU only)
├── qwen3:72b (40GB)    limit=2        ├── qwen3:8b (5GB)  limit=4  ├── qwen3:1.7b  limit=3
├── deepseek-r1:70b (45GB) limit=1     └── phi4:14b (9GB)  limit=3  └── phi4-mini   limit=5
└── available: 35GB                        available: 8GB

요청: model=qwen3:8b
  → Server B 선택 (모델 보유 + VRAM 여유)
  → limit=4, active=2 → 허용

요청: model=deepseek-r1:70b
  → Server A 선택 (유일하게 보유)
  → limit=1, active=1 → 큐 대기 (cold start 또는 AIMD 제한)

요청: model=qwen3:1.7b
  → Server C 선택 (모델 보유)
  → VRAM=0 (CPU) → Ollama에 위임, concurrency gate만 적용
```

**라우팅 우선순위**:
1. 해당 모델을 보유한 provider만 후보
2. model selection에서 enabled인 provider만
3. VRAM 여유가 많은 provider 우선
4. 동일 VRAM이면 paid tier key → non-free-tier provider 우선
5. Thermal/Circuit Breaker 통과 필수

### Phase 5: 새 모델 추가 시 동작

Ollama에 새 모델을 pull하면 다음 sync에서 자동 감지된다.

```
ollama pull llama3.3:70b  (Ollama 서버에서 직접)
  │
  ├── 다음 sync (≤300s)
  │   GET /api/tags → 새 모델 발견
  │   → ollama_models 테이블에 자동 추가
  │   → provider_selected_models에 is_enabled=true로 등록
  │
  ├── 첫 요청 도착
  │   → try_reserve: max_concurrent=1 (cold start, 학습 데이터 없음)
  │   → Ollama가 모델 자동 로드 → weight VRAM 점유
  │
  ├── 첫 sync with loaded model
  │   → /api/ps에서 weight 측정 → model_vram_profiles에 저장
  │   → /api/show에서 architecture 파싱 → KV cache 계산
  │   → baseline_tps 설정 (첫 throughput 데이터)
  │
  └── 이후 자동 학습
      → AIMD: sample ≥ 3부터 자동 조정
      → LLM Batch: 총 sample ≥ 10부터 전체 모델 조합 분석
```

**수동 개입이 필요한 경우**:
- 특정 모델을 특정 provider에서 비활성화: `PATCH /v1/providers/{id}/selected-models/{model} {is_enabled: false}`
- Probe 정책 변경: `PATCH /v1/dashboard/capacity/settings {probe_permits, probe_rate}`
- 즉시 sync 트리거: `POST /v1/providers/sync`

### 설정 참조

| 항목 | 기본값 | 위치 | 설명 |
|------|--------|------|------|
| sync_interval_secs | 300 | capacity_settings | 자동 sync 주기 |
| sync_enabled | true | capacity_settings | 자동 sync ON/OFF |
| analyzer_model | qwen2.5:3b | capacity_settings | LLM 분석용 모델 |
| probe_permits | 1 | capacity_settings | +N(위로 탐색), -N(아래로 탐색), 0=비활성 |
| probe_rate | 3 | capacity_settings | 매 N번 limit 도달 시 1회 probe |
| CAPACITY_ANALYZER_OLLAMA_URL | (provider URL) | env | LLM 분석 호출 대상 (분리 가능) |

---

## Background Loops

### Sync Loop (run_sync_loop — analyzer.rs)
- Tick: 30s, Cooldown: `capacity_settings.sync_interval_secs` (default 300s)
- Manual trigger: `POST /v1/providers/sync` (cooldown 무시)
- Per Ollama provider:
  1. `/api/version` → health check
  2. `/api/tags` → model sync (DB + Valkey cache)
  3. `/api/ps` → loaded model weight 측정
  4. `/api/show` → architecture 파싱 (hybrid Mamba+Attention 대응)
  5. throughput stats (PG) → KV per request 계산
  6. AIMD → max_concurrent 조정
  7. LLM batch → 전체 모델 조합 분석 (sample ≥ 10)
  8. DB persist → model_vram_profiles
- Gemini: not included (no VRAM concept)

### Health Checker (health_checker.rs)
- Interval: 30 seconds
- Ollama: covered by sync loop
- Gemini: `POST /v1beta/models/gemini-2.0-flash:generateContent` (minimal prompt)
- After hw_metrics load: `thermal.update(provider_id, temp_c)` → Normal/Soft/Hard
  - Sets/removes `veronex:throttle:{provider_id}` in Valkey (TTL 90s)

---

## Related Documents

- **VRAM pool + thermal + AIMD details**: `docs/llm/inference/capacity.md`
- **Streaming protocol + format conversion**: `docs/llm/providers/ollama-impl.md`
- **Ollama model sync**: `docs/llm/providers/ollama-models.md`
- **Web UI**: `docs/llm/frontend/pages/providers.md` -- OllamaTab + OllamaSyncSection
