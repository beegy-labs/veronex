# Hardware — GPU Server & Metrics

> SSOT | **Last Updated**: 2026-03-01 (rev: history max 168h → 1440h, adaptive buckets)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new metric to live node-exporter response | `infrastructure/outbound/hw_metrics.rs` → `fetch_node_metrics()` parsing + `NodeMetrics` struct |
| Add new metric to history chart | `gpu_server_handlers.rs` → `metrics_history()` ClickHouse SQL + `ServerMetricsPoint` struct |
| Add GPU server DB column | `migrations/` + `domain/entities/gpu_server.rs` + `persistence/gpu_server_registry.rs` |
| Change Prometheus HTTP SD response format | `gpu_server_handlers.rs` → `metrics_targets()` |
| Change history query bucket size | `gpu_server_handlers.rs` → `toStartOfInterval` SQL arg |
| Support new GPU vendor (e.g. NVIDIA) | `hw_metrics.rs` → add metric parsing branch alongside existing AMD section |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/gpu_server.rs` | `GpuServer` entity |
| `crates/inferq/src/application/ports/outbound/gpu_server_registry.rs` | `GpuServerRegistry` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/gpu_server_registry.rs` | Postgres impl |
| `crates/inferq/src/infrastructure/outbound/hw_metrics.rs` | `fetch_node_metrics()` — node-exporter parsing |
| `crates/inferq/src/infrastructure/inbound/http/gpu_server_handlers.rs` | GPU server CRUD + metrics handlers |

---

## Design Rationale

One physical server may run multiple Ollama backends (one per GPU). To avoid scraping
node-exporter multiple times per host, `GpuServer` is a separate entity from `LlmBackend`.

```
gpu_servers   (1 physical host = 1 node-exporter)
llm_backends  (1 Ollama process = 1 GPU)
  └── server_id → gpu_servers (nullable; Gemini = NULL)
```

---

## GpuServer Entity

```rust
// domain/entities/gpu_server.rs
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
       Live fetch from node-exporter (5s timeout)
       scrape_ok=false → unreachable; 422 → node_exporter_url not set
       → NodeMetrics

GET    /v1/servers/{id}/metrics/history?hours=N
       N: default 1, max 1440 (60 days); 503 = ClickHouse not configured
       → Vec<ServerMetricsPoint>  (adaptive buckets from otel_metrics_gauge)

GET    /v1/metrics/targets
       Prometheus HTTP SD — no auth required, OTel Collector only
       → [{ "targets": ["host:9100"], "labels": { server_id, server_name, host } }]
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

## ServerMetricsPoint (history response)

```rust
pub struct ServerMetricsPoint {
    pub ts: String,             // ISO 8601, 1-min bucket start
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,
    pub gpu_power_w: Option<f64>,
}
```

---

## node-exporter Metrics Parsed (hw_metrics.rs)

| Metric | Data |
|--------|------|
| `node_memory_MemTotal_bytes` | Total RAM |
| `node_memory_MemAvailable_bytes` | Available RAM |
| `node_cpu_seconds_total{cpu="N"}` | CPU core count |
| `node_hwmon_chip_names{chip_name="amdgpu"}` | AMD GPU chip ID |
| `node_drm_gpu_busy_percent{card="cardN"}` | GPU utilization (`--collector.drm`) |
| `node_drm_memory_vram_used_bytes` | VRAM used |
| `node_drm_memory_vram_total_bytes` | VRAM total |
| `node_hwmon_temp_celsius` | GPU temp (amdgpu only) |
| `node_hwmon_power_average_watt(s)` | GPU power (both spellings accepted) |

**Required flags**: `--collector.drm --collector.hwmon --collector.meminfo`

**AMD APU note (Ryzen AI Max+ 395)**: `chip` label is PCI address format (`0000:00:08_1_…`).
Identify via `node_hwmon_chip_names{chip_name="amdgpu"}`. Two-step query in `hw_metrics.rs`.

---

## History Bucket Sizes (adaptive)

| `hours` range | Bucket | Max points |
|--------------|--------|-----------|
| ≤ 24h | 1 MINUTE | 1 440 |
| ≤ 168h (7d) | 5 MINUTE | 2 016 |
| > 168h (up to 1440h / 60d) | 60 MINUTE | 1 440 |

Controlled by `let bucket_interval` in `gpu_server_handlers.rs` → passed into the SQL format string.

## ClickHouse History Query

```sql
-- Two-step: first find amdgpu chip label, then:
-- bucket_interval = "1 MINUTE" | "5 MINUTE" | "60 MINUTE" (selected by hours range)
SELECT toStartOfInterval(TimeUnix, INTERVAL {bucket_interval}) AS ts,
       maxIf(Value, MetricName='node_memory_MemTotal_bytes') / 1048576      AS mem_total_mb,
       avgIf(Value, MetricName='node_memory_MemAvailable_bytes') / 1048576  AS mem_avail_mb,
       avgIf(Value, MetricName='node_hwmon_temp_celsius'
             AND Attributes['chip'] = ? AND Attributes['sensor']='temp1')    AS gpu_temp_c,
       avgIf(Value, MetricName IN ('node_hwmon_power_average_watt',
             'node_hwmon_power_average_watts') AND Attributes['chip'] = ?)   AS gpu_power_w
FROM otel_metrics_gauge
WHERE Attributes['server_id'] = ? AND TimeUnix >= now() - INTERVAL ? HOUR
GROUP BY ts ORDER BY ts
```

`toStartOfInterval` returns `DateTime` (not `DateTime64`) → use `clickhouse::serde::time::datetime`.
`0.0` from `avgIf` with no data → converted to `None` in response.

---

## Web UI

→ See `docs/llm/frontend/web-backends.md` → ServersTab
