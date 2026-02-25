# Spec 15 — Backend Hardware Metrics

> SSOT | **Phase**: Phase 1 구현 완료 | **Last Updated**: 2026-02-25

## Goal

각 Ollama 백엔드(서버/파드)의 하드웨어 상태(GPU, RAM)를 inferq 대시보드에서
단일 화면으로 모니터링한다.

---

## 엔티티 구조

```
gpu_servers (물리 서버 = 1 node-exporter)
  id, name, node_exporter_url, registered_at

llm_backends (Ollama 프로세스 = 1 GPU 사용)
  server_id → gpu_servers (nullable, Gemini = NULL)
  gpu_index, total_vram_mb, url/api_key …

GET /v1/metrics/targets → gpu_servers 기반, 서버당 1 target (중복 없음)
GET /v1/servers/{id}/metrics → node-exporter 직접 fetch → CPU/메모리/GPU 라이브 데이터
```

**분리 이유**: 동일 물리 서버에 GPU 0/1 각각 Ollama 백엔드를 등록할 때
같은 node-exporter를 중복 scrape 하는 문제 해결.

**host/total_ram_mb 제거 이유**:
- `host`는 `node_exporter_url`에서 추출 가능 (중복)
- `total_ram_mb`는 node-exporter `node_memory_MemTotal_bytes`로 자동 수집됨 (수기 입력 불필요)

---

## Phase 1 — node-exporter + 라이브 fetch (구현 완료)

### 수집 파이프라인 (OTel 경로 — ClickHouse 장기 저장)

```
Ollama 서버 (각 머신):
  node-exporter :9100
       ↓ HTTP SD 등록 (inferq admin → GPU Servers 페이지)
  inferq /v1/metrics/targets   ← Prometheus HTTP SD endpoint (gpu_servers 기반)
       ↑ poll (30s)
  OTel Collector (prometheus receiver)
       ↓ fan-out
  ┌─ ClickHouse exporter     → otel_metrics_* (auto-schema, 장기 저장)
  └─ Kafka/Redpanda exporter → otel-metrics topic
                                    ↓ Kafka Engine consumer
                              ClickHouse otel_metrics_raw (원시 OTLP JSON 보존)
```

### 라이브 fetch 경로 (대시보드 실시간 표시)

```
inferq admin (GPU Servers 탭)
  ↓ useQuery (30s refetchInterval)
GET /v1/servers/{id}/metrics
  ↓ reqwest HTTP GET (5s timeout)
node-exporter :9100/metrics  (Prometheus text format)
  ↓ parse_prometheus_metrics()
NodeMetrics { mem_total_mb, mem_available_mb, cpu_cores, gpus:[{temp_c, power_w, vram_*}] }
  → UI: Memory bar / CPU cores / GPU temp·power·VRAM
```

### 파싱하는 node-exporter 메트릭

| 메트릭 | 출처 | 비고 |
|--------|------|------|
| `node_memory_MemTotal_bytes` | meminfo | 총 RAM |
| `node_memory_MemAvailable_bytes` | meminfo | 가용 RAM |
| `node_cpu_seconds_total{cpu="N"}` | cpu | 코어 수 (unique cpu label 집합) |
| `node_hwmon_chip_names{chip="..",chip_name="amdgpu"}` | hwmon | AMD GPU 칩 식별 (APU 대응) |
| `node_drm_gpu_busy_percent{card="cardN"}` | drm | GPU 사용률 % |
| `node_drm_memory_vram_used_bytes{card="cardN"}` | drm | VRAM 사용 |
| `node_drm_memory_vram_total_bytes{card="cardN"}` | drm | VRAM 총량 |
| `node_hwmon_temp_celsius{chip=".."}` | hwmon | GPU 온도 (amdgpu 칩만) |
| `node_hwmon_power_average_watt{chip=".."}` | hwmon | GPU 전력 (trailing `s` 유무 모두 허용) |

> node-exporter 활성화 필요 플래그: `--collector.drm` (AMD GPU sysfs), `--collector.hwmon` (온도/전력), `--collector.meminfo` (RAM, 기본 활성화)

**AMD APU 주의사항 (Ryzen AI Max+ 395 등)**:
- `chip` label이 `amdgpu-pci-xxxx` 형식이 아닌 PCI 주소 형식(`0000:00:08_1_0000:c4:00_0`)으로 노출됨
- `node_hwmon_chip_names{chip_name="amdgpu"}` 메트릭으로 amdgpu 칩 식별 (파서가 두 방식 모두 지원)
- DRM `--collector.drm` 미활성화 시 VRAM/busy% 없이 온도·전력만 표시
- `node_hwmon_power_average_watt` (trailing `s` 없음) 형식 사용 가능

### dispatch 로직 (Phase 1)

```
1. total_vram_mb == 0  → VRAM 무제한, 항상 dispatch 가능
2. total_vram_mb > 0   → Ollama /api/ps로 현재 로드된 모델 VRAM 합산
                         available = total_vram_mb - used_vram
                         available 최대인 서버로 dispatch
3. 온도 경보           → Phase 2 (sidecar) 에서 처리
```

---

## Phase 2 — inferq-agent sidecar (예정, 미구현)

```
Ollama Pod/서버:
  ┌─ ollama
  ├─ inferq-agent  ← sidecar
  │    GET /api/metrics → GPU temp/VRAM, pod cgroup RAM, Ollama /api/ps
  └─ node-exporter ← 노드 전체 메트릭

health_checker (30s) → Valkey 캐시 → dispatcher (온도 guard, 실시간 VRAM)
inferq-agent         → OTel → ClickHouse → dashboard
```

**Phase 2 추가 기능:**
- GPU 온도 >= 85°C 시 dispatch 차단
- Pod 단위 RAM 사용량 (cgroup `/sys/fs/cgroup/memory.current`)
- Valkey 캐시 기반 실시간 VRAM dispatch

---

## DB 스키마

```sql
-- migration 20260225000009: gpu_servers (물리 서버 엔티티)
CREATE TABLE IF NOT EXISTS gpu_servers (
    id                UUID         PRIMARY KEY,
    name              VARCHAR(255) NOT NULL,
    host              TEXT         NOT NULL,     -- → 000012에서 DROP
    node_exporter_url TEXT,
    total_ram_mb      BIGINT       NOT NULL DEFAULT 0,  -- → 000013에서 DROP
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- migration 20260225000010: llm_backends → gpu_servers FK
ALTER TABLE llm_backends
    ADD COLUMN IF NOT EXISTS server_id UUID REFERENCES gpu_servers(id) ON DELETE SET NULL;

-- migration 20260225000011: node-exporter, RAM 필드 llm_backends에서 제거
ALTER TABLE llm_backends
    DROP COLUMN IF EXISTS node_exporter_url,
    DROP COLUMN IF EXISTS total_ram_mb;

-- migration 20260225000012: host 제거 (node_exporter_url에서 추출 가능)
ALTER TABLE gpu_servers DROP COLUMN IF EXISTS host;

-- migration 20260225000013: total_ram_mb 제거 (node-exporter가 자동 제공)
ALTER TABLE gpu_servers DROP COLUMN IF EXISTS total_ram_mb;
```

**최종 gpu_servers 스키마:**
```sql
CREATE TABLE gpu_servers (
    id                UUID         PRIMARY KEY,
    name              VARCHAR(255) NOT NULL,
    node_exporter_url TEXT,
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

---

## 엔티티

```rust
pub struct GpuServer {
    pub id: Uuid,
    pub name: String,
    pub node_exporter_url: Option<String>, // "http://192.168.1.10:9100"
    pub registered_at: DateTime<Utc>,
}

pub struct LlmBackend {
    pub id: Uuid,
    pub name: String,
    pub backend_type: BackendType,         // Ollama | Gemini
    pub url: String,
    pub api_key_encrypted: Option<String>,
    pub is_active: bool,
    pub total_vram_mb: i64,               // 수기 입력, 0 = 미등록
    pub gpu_index: Option<i16>,           // 수기 입력
    pub server_id: Option<Uuid>,          // FK → gpu_servers
    pub agent_url: Option<String>,        // Phase 2용, 현재 미사용
    pub status: LlmBackendStatus,
    pub registered_at: DateTime<Utc>,
}

// 라이브 fetch 응답
pub struct NodeMetrics {
    pub scrape_ok: bool,
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    pub cpu_cores: u32,
    pub gpus: Vec<GpuNodeMetrics>,
}

pub struct GpuNodeMetrics {
    pub card: String,           // "card0"
    pub temp_c: Option<f64>,    // hwmon
    pub power_w: Option<f64>,   // hwmon
    pub vram_used_mb: Option<u64>, // drm
    pub vram_total_mb: Option<u64>, // drm
    pub busy_pct: Option<f64>,  // drm
}
```

---

## API 엔드포인트

```
# GPU Server 관리 (인증 필요)
POST   /v1/servers            RegisterGpuServerRequest { name, node_exporter_url? }
GET    /v1/servers            → Vec<GpuServerSummary>
DELETE /v1/servers/{id}       → 204

# 라이브 하드웨어 메트릭 (인증 필요)
GET    /v1/servers/{id}/metrics          → NodeMetrics (node-exporter 직접 fetch)
  scrape_ok=false  → node-exporter 연결 불가
  422              → node_exporter_url 미설정

GET    /v1/servers/{id}/metrics/history  → Vec<ServerMetricsPoint> (ClickHouse 이력)
  ?hours=N  (기본 1, 최대 168)
  503       → ClickHouse 미설정
  1-minute 버킷 집계, server_id 기반 otel_metrics_gauge 조회

# Prometheus HTTP SD (인증 불필요 — OTel Collector 호환)
GET    /v1/metrics/targets    → gpu_servers 기반 SD targets

# LLM Backend 관리 (인증 필요)
POST   /v1/backends           RegisterBackendRequest { name, backend_type, url?, api_key?, total_vram_mb?, gpu_index?, server_id? }
GET    /v1/backends           → Vec<BackendSummary>
PATCH  /v1/backends/{id}      UpdateBackendRequest { name, url?, api_key?, total_vram_mb?, gpu_index?, server_id? }
  gpu_index / server_id = null → DB에서 NULL로 업데이트 (연결 해제)
  api_key = "" 또는 null → 기존 키 유지
DELETE /v1/backends/{id}      → 204
```

#### ServerMetricsPoint (metrics/history 응답 항목)
```rust
pub struct ServerMetricsPoint {
    pub ts: String,          // ISO 8601, 1분 버킷 시작
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,  // None = 해당 버킷에 GPU 데이터 없음
    pub gpu_power_w: Option<f64>,
}
```

SD 응답 포맷:
```json
[
  {
    "targets": ["192.168.1.10:9100"],
    "labels": {
      "server_id": "...",
      "server_name": "gpu-node-1",
      "host": "192.168.1.10"
    }
  }
]
```

- `node_exporter_url`이 등록된 gpu_servers만 포함
- 동일 서버에 백엔드 여러 개 등록해도 target은 1개 (중복 없음)
- `http://` scheme 제거 → `host:port` 형식으로 반환; `host` 라벨은 URL에서 추출

---

## ClickHouse 테이블

```
otel_metrics_gauge   ← OTel Collector ClickHouse exporter (직접 기입, 주 쿼리 테이블)
otel_metrics_sum     ← OTel Collector ClickHouse exporter
otel_metrics_*       ← OTel Collector ClickHouse exporter

otel_metrics_raw     ← Kafka Engine consumer (Redpanda → raw OTLP JSON, 아카이브용)
otel_traces_raw      ← Kafka Engine consumer
```

**`otel_metrics_gauge` 컬럼 (대시보드 히스토리 쿼리 기준)**:

| 컬럼 | 타입 | 설명 |
|------|------|------|
| `MetricName` | String | `node_memory_MemTotal_bytes`, `node_hwmon_temp_celsius` 등 |
| `Attributes` | Map(String,String) | `server_id`, `chip`, `sensor`, `host` 등 레이블 |
| `Value` | Float64 | 메트릭 값 |
| `TimeUnix` | DateTime64(9) | 수집 타임스탬프 |

`GET /v1/servers/{id}/metrics/history` 쿼리 패턴:
```sql
-- 1. amdgpu 칩 레이블 조회 (PCI 주소 형식 대응)
SELECT DISTINCT Attributes['chip']
FROM otel_metrics_gauge
WHERE MetricName = 'node_hwmon_chip_names'
  AND Attributes['chip_name'] = 'amdgpu'
  AND Attributes['server_id'] = ?
LIMIT 1

-- 2. 1분 버킷 피벗
SELECT toStartOfInterval(TimeUnix, INTERVAL 1 MINUTE) AS ts,
       maxIf(Value, MetricName = 'node_memory_MemTotal_bytes') / 1048576 AS mem_total_mb,
       avgIf(Value, MetricName = 'node_memory_MemAvailable_bytes') / 1048576 AS mem_avail_mb,
       avgIf(Value, MetricName = 'node_hwmon_temp_celsius'
           AND Attributes['chip'] = ? AND Attributes['sensor'] = 'temp1') AS gpu_temp_c,
       avgIf(Value, MetricName IN ('node_hwmon_power_average_watt','node_hwmon_power_average_watts')
           AND Attributes['chip'] = ?) AS gpu_power_w
FROM otel_metrics_gauge
WHERE Attributes['server_id'] = ?
  AND TimeUnix >= now() - INTERVAL ? HOUR
GROUP BY ts ORDER BY ts
```

**`node_metrics`** (미래: MV로 `otel_metrics_gauge`에서 자동 채움):
```sql
node_metrics (ts, instance, gpu_index, gpu_vram_*, gpu_util_ratio,
              gpu_temp_celsius, gpu_power_watts, mem_total_bytes, mem_available_bytes)
```

---

## Dashboard 표시 (Phase 1)

### GPU Servers 테이블 (30s 자동 갱신)

```
Name         node-exporter endpoint       Live Metrics                         Actions
──────────────────────────────────────────────────────────────────────────────────────
gpu-node-1   http://192.168.1.10:9100     MEM  28.5 / 64.0 GB · 44%           [📊][🗑]
                                          CPU  32 cores
                                          GPU  card0 · 🌡32°C · ⚡10W
gpu-node-2   not configured               —                                    [📊][🗑]
```

- MEM/CPU/GPU 레이블 칼럼 정렬로 가독성 향상
- GPU 온도 ≥ 85°C → 빨간색 강조
- node-exporter 연결 불가 → "unreachable" 배지 + 재시도 버튼
- [📊] 클릭 → ClickHouse 이력 모달 (시간 범위: 1h/3h/6h/24h + Sync 버튼)

### LLM Backends 테이블

```
Backend                  Assignment                  Status    Registered   Actions
──────────────────────────────────────────────────────────────────────────────────────
gpu-ollama-1  [ollama]   gpu-node-1                  online    Mar 1, 2026  [↺][↻][✏][🗑]
192.168.1.10:11434       GPU 0 · VRAM 64 GB

gemini-prod   [gemini]   cloud API                   online    Mar 1, 2026  [↺][✏][🗑]
```

**열 구성 (5열, 기존 8열에서 축소):**
- **Backend**: 이름 + 타입 배지 + URL(Ollama) 부제목
- **Assignment**: 연결된 GPU Server · GPU Index · Max VRAM (Ollama), "cloud API" (Gemini)
- **Status**: online/degraded/offline 배지
- **Registered**: 등록일
- **Actions**: healthcheck · sync models(Ollama만) · edit · delete

**GPU Index 드롭다운**: 선택된 GPU Server에서 라이브 메트릭 fetch → 카드 목록 자동 표시
**Max VRAM 입력**: MiB / GiB 단위 토글 버튼 (내부는 항상 MiB)

---

## 구현 체크리스트

### Phase 1
- [x] `total_vram_mb` 수기 입력 (기존)
- [x] `gpu_index` 필드: migration + entity + persistence + handler + web UI
- [x] `GpuServer` 엔티티: domain + ports + persistence + HTTP handlers + web UI
- [x] `GpuServerRegistry` trait + `PostgresGpuServerRegistry` 구현
- [x] `POST/GET/DELETE /v1/servers` 엔드포인트
- [x] `host` 필드 제거 (migration 000012): node_exporter_url에서 추출
- [x] `total_ram_mb` 필드 제거 (migration 000013): node-exporter 자동 수집
- [x] `GET /v1/servers/{id}/metrics` — node-exporter 직접 fetch + Prometheus 파싱
- [x] `NodeMetrics` 구조체: mem / cpu_cores / GPU(temp, power, VRAM, busy%)
- [x] AMD APU hwmon 파서 수정: `node_hwmon_chip_names` 기반 칩 식별 (PCI 주소 형식 chip label 대응)
- [x] `node_hwmon_power_average_watt` trailing `s` 유무 모두 허용
- [x] `GET /v1/metrics/targets` — gpu_servers 기반 (서버당 1 target, 중복 없음)
- [x] CORS: `Method::PATCH` allow_methods에 추가 (Edit Backend 모달 PATCH 요청 정상화)
- [x] Web: GPU Servers 섹션 — Live Metrics 컬럼 (30s 자동 갱신, unreachable 배지)
- [x] Web: Register GPU Server 폼 — name + node_exporter_url 만
- [x] Web: Register Backend 폼 — server_id 드롭다운
- [x] Web: Edit Backend 버튼 + 모달 (name, url, api_key, vram, gpu_index, server_id 수정)
- [x] `PATCH /v1/backends/{id}` — 수정 가능 필드 업데이트, null로 gpu_index/server_id 해제 가능
- [x] `docker-compose.ollama.yml` (Ollama + node-exporter 번들, GPU 서버 측)
- [x] OTel Collector prometheus receiver (HTTP SD 기반 동적 targets)
- [x] OTel → Redpanda fan-out 파이프라인
- [x] ClickHouse Kafka Engine consumer tables (otel_metrics_raw, otel_traces_raw)
- [x] ClickHouse `node_metrics` 테이블 스키마 정의 (미래 MV용)
- [x] `GET /v1/servers/{id}/metrics/history?hours=N` — ClickHouse `otel_metrics_gauge` 1분 버킷 조회
- [x] `ServerMetricsPoint` 응답 구조체: ts / mem_total_mb / mem_avail_mb / gpu_temp_c / gpu_power_w
- [x] Web: GPU Server 행 BarChart 버튼 → ClickHouse 이력 모달 (Memory%, GPU Temp, GPU Power 차트)
- [x] Web: 시간 범위 선택 (1h/3h/6h/24h) + Sync 버튼 (수동 갱신)
- [x] Web: GPU Index 드롭다운 — 선택 서버 라이브 메트릭 기반 card 목록; SelectItem value="none" 센티널 사용 (Radix UI 빈 문자열 금지 대응)
- [x] Web: Max VRAM MiB/GiB 단위 토글 입력 (VramInput 컴포넌트)
- [x] Web: Backends 테이블 8열 → 5열 재구성 (Backend / Assignment / Status / Registered / Actions); 가독성 개선

### Phase 2 (사이드카, 미정)
- [ ] `inferq-agent` crate (GPU sysfs + cgroup + Ollama /api/ps)
- [ ] `agent_url` 필드 활성화
- [ ] health_checker agent polling → Valkey
- [ ] 온도 guard dispatch
- [ ] K8s sidecar manifest
