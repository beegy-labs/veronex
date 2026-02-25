# Hardware — GPU Server & Metrics Pipeline

> SSOT | **Last Updated**: 2026-02-25

## 설계 원칙

동일 물리 서버에 Ollama 백엔드 여러 개(GPU 0/1)를 등록할 때
node-exporter를 중복 scrape하는 문제를 방지하기 위해 `gpu_servers` 엔티티를 분리.

```
gpu_servers   (물리 서버 = 1 node-exporter)
llm_backends  (Ollama 프로세스 = 1 GPU)
  └── server_id → gpu_servers (nullable, Gemini = NULL)
```

## GpuServer Entity

```rust
pub struct GpuServer {
    pub id: Uuid,
    pub name: String,
    pub node_exporter_url: Option<String>, // "http://192.168.1.10:9100"
    pub registered_at: DateTime<Utc>,
}
```

## DB Schema

```sql
CREATE TABLE gpu_servers (
    id                UUID         PRIMARY KEY,
    name              VARCHAR(255) NOT NULL,
    node_exporter_url TEXT,
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
-- migrations: 000009 CREATE, 000012 DROP host, 000013 DROP total_ram_mb
-- (host → node_exporter_url에서 추출; total_ram_mb → node-exporter 자동 수집)
```

## API Endpoints

```
POST   /v1/servers             RegisterGpuServerRequest { name, node_exporter_url? }
GET    /v1/servers             → Vec<GpuServerSummary>
DELETE /v1/servers/{id}        → 204

GET    /v1/servers/{id}/metrics          → NodeMetrics  (node-exporter 직접 fetch, 5s timeout)
  scrape_ok=false  → node-exporter 연결 불가
  422              → node_exporter_url 미설정

GET    /v1/servers/{id}/metrics/history  → Vec<ServerMetricsPoint>
  ?hours=N  (기본 1, 최대 168)
  503       → ClickHouse 미설정
  1-minute 버킷 집계, ClickHouse otel_metrics_gauge 조회

GET    /v1/metrics/targets     → Prometheus HTTP SD (인증 불필요, OTel Collector 전용)
```

### NodeMetrics (라이브 fetch 응답)

```rust
pub struct NodeMetrics {
    pub scrape_ok: bool,
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    pub cpu_cores: u32,
    pub gpus: Vec<GpuNodeMetrics>,
}

pub struct GpuNodeMetrics {
    pub card: String,              // "card0"
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub busy_pct: Option<f64>,
}
```

### ServerMetricsPoint (history 응답)

```rust
pub struct ServerMetricsPoint {
    pub ts: String,               // ISO 8601, 1분 버킷 시작
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,
    pub gpu_power_w: Option<f64>,
}
```

### Prometheus HTTP SD 응답 형식

```json
[
  {
    "targets": ["192.168.1.10:9100"],
    "labels": { "server_id": "...", "server_name": "gpu-node-1", "host": "192.168.1.10" }
  }
]
```

- `node_exporter_url` 등록된 gpu_servers만 포함
- 동일 서버에 백엔드 여러 개 등록해도 target 1개 (중복 없음)

---

## node-exporter 파싱 메트릭

| 메트릭 | 수집 정보 | 비고 |
|--------|-----------|------|
| `node_memory_MemTotal_bytes` | 총 RAM | |
| `node_memory_MemAvailable_bytes` | 가용 RAM | |
| `node_cpu_seconds_total{cpu="N"}` | 코어 수 | unique cpu label 집합 |
| `node_hwmon_chip_names{chip_name="amdgpu"}` | AMD GPU 칩 식별 | APU 대응 |
| `node_drm_gpu_busy_percent{card="cardN"}` | GPU 사용률 | `--collector.drm` 필요 |
| `node_drm_memory_vram_used_bytes` | VRAM 사용 | `--collector.drm` 필요 |
| `node_drm_memory_vram_total_bytes` | VRAM 총량 | `--collector.drm` 필요 |
| `node_hwmon_temp_celsius` | GPU 온도 | amdgpu 칩만 |
| `node_hwmon_power_average_watt(s)` | GPU 전력 | trailing `s` 유무 모두 허용 |

필수 node-exporter 플래그: `--collector.drm --collector.hwmon --collector.meminfo`

### AMD APU 주의사항 (Ryzen AI Max+ 395)

- `chip` label이 PCI 주소 형식(`0000:00:08_1_0000:c4:00_0`)으로 노출됨
- `node_hwmon_chip_names{chip_name="amdgpu"}` 메트릭으로 amdgpu 칩 식별
- `--collector.drm` 미활성화 시 VRAM/busy% 없이 온도·전력만 표시

---

## OTel 메트릭 파이프라인

```
GPU 서버 (docker-compose.ollama.yml)
  node-exporter :9100
       ↓ HTTP SD 등록 (inferq admin → GPU Servers 페이지)
  inferq /v1/metrics/targets   ← Prometheus HTTP SD endpoint
       ↑ poll 30s
  OTel Collector (prometheus receiver)
       ↓ fan-out
  ┌─ ClickHouse exporter   → otel_metrics_* (자동 스키마)
  └─ kafka/metrics          → redpanda:9092 → ClickHouse otel_metrics_raw
```

### ClickHouse 쿼리 (`otel_metrics_gauge`)

```sql
-- 1분 버킷 history
SELECT toStartOfInterval(TimeUnix, INTERVAL 1 MINUTE) AS ts,
       maxIf(Value, MetricName = 'node_memory_MemTotal_bytes') / 1048576    AS mem_total_mb,
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

---

## Web UI (admin `/backends`)

### GPU Servers 테이블

```
Name         node-exporter endpoint       Live Metrics (30s auto-refresh)   Actions
─────────────────────────────────────────────────────────────────────────────────────
gpu-node-1   http://192.168.1.10:9100     MEM 28.5 / 64.0 GB · 44%         [📊][🗑]
                                          CPU 32 cores
                                          GPU card0 · 32°C · 10W
gpu-node-2   not configured               —                                  [📊][🗑]
```

- GPU 온도 ≥ 85°C → 빨간색 강조
- node-exporter 연결 불가 → "unreachable" 배지 + 재시도 버튼
- [📊] 클릭 → ClickHouse 이력 모달 (1h/3h/6h/24h + Sync 버튼)

---

## Phase 2 — inferq-agent sidecar (예정, 미구현)

```
Ollama Pod:
  ├── ollama
  ├── inferq-agent  ← sidecar
  │     GET /api/metrics → GPU temp/VRAM, cgroup RAM, Ollama /api/ps
  └── node-exporter

health_checker → Valkey 캐시 → dispatcher (온도 guard, 실시간 VRAM)
```

추가 예정 기능:
- GPU 온도 ≥ 85°C 시 dispatch 차단
- Pod 단위 RAM (cgroup `/sys/fs/cgroup/memory.current`)
- `agent_url` 필드 활성화
