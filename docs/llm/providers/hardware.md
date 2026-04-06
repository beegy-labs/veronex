# Hardware — GPU Server & Metrics

> SSOT | **Last Updated**: 2026-03-21 (rev: cpu_usage_pct in ServerMetricsPoint, counter delta CTE for CPU usage history)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new metric to live node-exporter response | `infrastructure/outbound/hw_metrics.rs` → `fetch_node_metrics()` parsing + `NodeMetrics` struct |
| Add new metric to history chart | `gpu_server_handlers.rs` → `metrics_history()` ClickHouse SQL + `ServerMetricsPoint` struct |
| Add GPU server DB column | `migrations/` + `domain/entities/mod.rs` + `persistence/gpu_server_registry.rs` |
| Change Prometheus HTTP SD response format | `gpu_server_handlers.rs` → `metrics_targets()` |
| Change history query bucket size | `gpu_server_handlers.rs` → `toStartOfInterval` SQL arg |
| Support new GPU vendor (e.g. NVIDIA) | `hw_metrics.rs` → add metric parsing branch alongside existing AMD section |
| Change thermal thresholds | `capacity/thermal.rs` → `ThermalThresholds` presets or `set_thresholds()` API |
| Understand thermal state machine | `capacity/thermal.rs` → 5-state: Normal/Soft/Hard/Cooldown/RampUp |
| Adjust cooldown duration | `capacity/thermal.rs` → `COOLDOWN_SECS` (default 300s) |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/mod.rs` | `GpuServer` entity |
| `crates/veronex/src/application/ports/outbound/gpu_server_registry.rs` | `GpuServerRegistry` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/gpu_server_registry.rs` | Postgres impl |
| `crates/veronex/src/infrastructure/outbound/hw_metrics.rs` | `fetch_node_metrics()` — node-exporter parsing |
| `crates/veronex/src/infrastructure/inbound/http/gpu_server_handlers.rs` | GPU server CRUD + metrics handlers |

---

## Design Rationale

One physical server may run multiple Ollama providers (one per GPU). To avoid scraping
node-exporter multiple times per host, `GpuServer` is a separate entity from `LlmProvider`.

```
gpu_servers   (1 physical host = 1 node-exporter)
llm_providers (1 Ollama process = 1 GPU)
  └── server_id → gpu_servers (nullable; Gemini = NULL)
```

---

## GpuServer Entity

```rust
// domain/entities/mod.rs
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
-- migrations: 000009 CREATE, 000012 drop host, 000013 drop total_ram_mb
```

---

## API Endpoints (gpu_server_handlers.rs)

```
POST   /v1/servers             RegisterGpuServerRequest → GpuServerSummary
GET    /v1/servers             → Vec<GpuServerSummary>
PATCH  /v1/servers/{id}        UpdateGpuServerRequest → 200
DELETE /v1/servers/{id}        → 204

GET    /v1/servers/{id}/metrics
       Reads cached NodeMetrics from Valkey (written by health_checker every 30s cycle).
       Returns default (empty) NodeMetrics when Valkey is unavailable or no cached data exists.
       No live node-exporter scrape — scales to 10K+ providers.
       → NodeMetrics

GET    /v1/servers/{id}/metrics/history?hours=N
       N: default 1, max 1440 (60 days); 503 = ClickHouse not configured
       → Vec<ServerMetricsPoint>  (adaptive buckets from otel_metrics_gauge)

GET    /v1/metrics/targets
       Agent target discovery — no auth, returns two target types (server + ollama)
       → [{ "targets": ["host:9100"], "labels": { type, server_id, server_name } },
          { "targets": ["host:11434"], "labels": { type, provider_id, provider_name, server_id? } }]
```

### Request Structs

```rust
pub struct RegisterGpuServerRequest { pub name: String, pub node_exporter_url: Option<String> }
pub struct UpdateGpuServerRequest   { pub name: Option<String>, pub node_exporter_url: Option<String> }
```

---

## NodeMetrics (live fetch response)

```rust
pub struct NodeMetrics {
    pub scrape_ok: bool,
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    pub cpu_logical: u32,           // logical threads from node_cpu_seconds_total
    pub cpu_physical: Option<u32>,  // physical cores from node_cpu_info (None if not exported)
    pub cpu_usage_pct: Option<f64>, // instantaneous CPU % (None on first scrape — delta required)
    pub gpus: Vec<GpuNodeMetrics>,
}
pub struct GpuNodeMetrics {
    pub card: String,              // "card0"
    pub temp_c: Option<f64>,       // edge (temp1)
    pub temp_junction_c: Option<f64>, // hotspot (temp2) — primary throttle input
    pub temp_mem_c: Option<f64>,   // VRAM (temp3) — data corruption guard
    pub power_w: Option<f64>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub busy_pct: Option<f64>,
}
```

## ServerMetricsPoint (history response)

```rust
pub struct ServerMetricsPoint {
    pub ts: String,             // ISO 8601, 1-min bucket start
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub cpu_usage_pct: Option<f64>,       // CPU usage % (counter delta via neighbor() CTE)
    pub gpu_temp_c: Option<f64>,          // edge
    pub gpu_temp_junction_c: Option<f64>, // junction/hotspot
    pub gpu_temp_mem_c: Option<f64>,      // VRAM
    pub gpu_power_w: Option<f64>,
}
```

---

## node-exporter Metrics Parsed (hw_metrics.rs)

| Metric | Data |
|--------|------|
| `node_memory_MemTotal_bytes` | Total RAM |
| `node_memory_MemAvailable_bytes` | Available RAM |
| `node_cpu_seconds_total{cpu="N"}` | Logical CPU count + usage delta |
| `node_cpu_info{cpu,core,package}` | Physical core count (optional, `--collector.cpu.info`) |
| `node_hwmon_chip_names{chip_name="amdgpu"}` | AMD GPU chip ID |
| `node_drm_gpu_busy_percent{card="cardN"}` | GPU utilization (`--collector.drm`) |
| `node_drm_memory_vram_used_bytes` | VRAM used |
| `node_drm_memory_vram_size_bytes` | VRAM total (older node-exporter) |
| `node_drm_memory_vram_total_bytes` | VRAM total (newer node-exporter; overwrites `size_bytes` if both present) |
| `node_hwmon_temp_celsius{sensor="temp1"}` | GPU edge temp (amdgpu only) |
| `node_hwmon_temp_celsius{sensor="temp2"}` | GPU junction/hotspot temp |
| `node_hwmon_temp_celsius{sensor="temp3"}` | GPU memory (HBM/GDDR) temp |
| `node_hwmon_power_average_watt(s)` | GPU power (both spellings accepted) |

**Required flags**: `--collector.drm --collector.hwmon --collector.meminfo`

**AMD APU note (Ryzen AI Max+ 395)**: `chip` label is PCI address format (`0000:00:08_1_…`).
Identify via `node_hwmon_chip_names{chip_name="amdgpu"}`. Two-step query in `hw_metrics.rs`.

## GPU Vendor Detection via node-exporter

GPU vendor is inferred from **DRM metric presence** — amdgpu kernel driver exposes DRM metrics via node-exporter; NVIDIA uses proprietary driver and does not:

| Condition | `gpu_vendor` | Thermal Profile |
|-----------|-------------|-----------------|
| DRM GPU metrics present (`node_drm_*`) | `"amd"` | CPU (75/82/90°C) — covers AMD discrete + Ryzen AI APU |
| No DRM GPU metrics | `""` (empty) | CPU (default) |

The `gpu_vendor` field is set in `health_checker` (not from sysfs vendor IDs), cached in Valkey (`HwMetrics`), and used to call `thermal.set_thresholds()` every 30s cycle.

**Note**: NVIDIA GPU thermal profile (`GPU`: 80/88/93°C) is defined but currently unreachable — NVIDIA does not expose DRM metrics via node-exporter. NVIDIA support requires nvidia-smi integration (future).

**GPU metrics by driver**:
- **AMD (amdgpu)**: Full DRM support — VRAM used/total, GPU busy %, hwmon temp (edge/junction/mem), power
- **NVIDIA**: No DRM metrics → `gpu_vendor=""` → CPU thermal profile applied as fallback


---

## Service Health Monitoring (health_checker extension)

The health checker (30s loop) also probes core infrastructure services and stores per-pod results in Valkey.

### Probed Services

| Service | Probe | Timeout |
|---------|-------|---------|
| PostgreSQL | `SELECT 1` | 3s |
| Valkey | `PING` | instant |
| ClickHouse | `GET {ANALYTICS_URL}/health` | 3s |
| S3/MinIO | `GET {S3_ENDPOINT}/minio/health/live` | 3s |
| Vespa | `GET {VESPA_URL}/state/v1/health` | 3s |

Vespa probe runs only when `VESPA_URL` env var is set. `check_and_store_services(vespa_url: Option<&str>)`.

### Storage

```
Key:    veronex:svc:health:{instance_id}   (HASH, TTL=60s)
Fields: postgresql, valkey, clickhouse, s3, vespa (when VESPA_URL set)
Value:  {"s":"ok","ms":3,"t":1711699200000}
```

Each API pod writes to its own key (no HPA write conflicts). Dead pod's key expires via TTL.

### API

```
GET /v1/dashboard/services → ServiceHealthResponse
```

Merges all pods' perspectives: any "ok" = ok, mixed = degraded, all error = unavailable.
API pod liveness from `veronex:heartbeat:{id}` TTL. Agent pod liveness from provider heartbeat sharding.

### Key Files

| File | Purpose |
|------|---------|
| `health_checker.rs` | `check_and_store_services()` — probes + Valkey HASH write |
| `dashboard_handlers.rs` | `get_service_health()` — merge + respond |
| `valkey_keys.rs` | `service_health(instance_id)` key function |

→ See `hardware-metrics.md` for history buckets, ClickHouse query, thermal state machine, and web UI.
