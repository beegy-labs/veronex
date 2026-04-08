# Hardware — GPU Server & Metrics

> SSOT | **Last Updated**: 2026-03-21 (rev: cpu_usage_pct in ServerMetricsPoint, counter delta CTE for CPU usage history)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new metric to live node-exporter response | `infrastructure/outbound/hw_metrics.rs` → `fetch_node_metrics()` parsing + `NodeMetrics` struct |
| Add new metric to history chart | `gpu_server_handlers.rs` → `metrics_history()` ClickHouse SQL + `ServerMetricsPoint` struct |
| Add GPU server DB column | `docker/postgres/init.sql` + `domain/entities/mod.rs` + `persistence/gpu_server_registry.rs` |
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

→ `hardware-impl.md` — node-exporter metrics parsed, GPU vendor detection, service health monitoring
→ `hardware-metrics.md` — history buckets, ClickHouse query, thermal state machine, web UI
