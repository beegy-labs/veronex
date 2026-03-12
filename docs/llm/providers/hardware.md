# Hardware — GPU Server & Metrics

> SSOT | **Last Updated**: 2026-03-12 (rev: 5-state thermal machine, Cooldown/RampUp)

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
       Live fetch from node-exporter (5s timeout)
       scrape_ok=false → unreachable; 422 → node_exporter_url not set
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
    pub cpu_cores: u32,
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
| `node_cpu_seconds_total{cpu="N"}` | CPU core count |
| `node_hwmon_chip_names{chip_name="amdgpu"}` | AMD GPU chip ID |
| `node_drm_gpu_busy_percent{card="cardN"}` | GPU utilization (`--collector.drm`) |
| `node_drm_memory_vram_used_bytes` | VRAM used |
| `node_drm_memory_vram_total_bytes` | VRAM total |
| `node_hwmon_temp_celsius{sensor="temp1"}` | GPU edge temp (amdgpu only) |
| `node_hwmon_temp_celsius{sensor="temp2"}` | GPU junction/hotspot temp |
| `node_hwmon_temp_celsius{sensor="temp3"}` | GPU memory (HBM/GDDR) temp |
| `node_hwmon_power_average_watt(s)` | GPU power (both spellings accepted) |

**Required flags**: `--collector.drm --collector.hwmon --collector.meminfo`

**AMD APU note (Ryzen AI Max+ 395)**: `chip` label is PCI address format (`0000:00:08_1_…`).
Identify via `node_hwmon_chip_names{chip_name="amdgpu"}`. Two-step query in `hw_metrics.rs`.

## GPU Vendor Detection via node-exporter

GPU vendor is detected from sysfs `/sys/class/drm/cardN/device/vendor` via node-exporter metrics:

| Vendor ID | `gpu_vendor` | Thermal Profile |
|-----------|-------------|-----------------|
| `0x1002` | `"amd"` | CPU (75/82/90°C) — Ryzen AI 395+ = APU/iGPU |
| `0x10de` | `"nvidia"` | GPU (80/88/93°C) |
| other/none | `"unknown"` | CPU (default) |

The `gpu_vendor` field is derived from node-exporter metrics and cached in `HwMetrics`. The health_checker reads it every 30s cycle and calls `thermal.set_thresholds()` to configure per-provider thermal limits.

**Agent GPU metrics by vendor**:
- **AMD**: Full sysfs support — `mem_info_vram_used`, `mem_info_vram_total`, `gpu_busy_percent`, hwmon temp/power
- **NVIDIA**: Card detection only via sysfs vendor ID. Detailed VRAM/temp metrics require nvidia-smi integration (future)

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
SELECT toStartOfInterval(ts, INTERVAL {bucket_interval}) AS ts,
       maxIf(value, metric_name='node_memory_MemTotal_bytes') / 1048576      AS mem_total_mb,
       avgIf(value, metric_name='node_memory_MemAvailable_bytes') / 1048576  AS mem_avail_mb,
       avgIf(value, metric_name='node_hwmon_temp_celsius'
             AND attributes['chip'] = ? AND attributes['sensor']='temp1')    AS gpu_temp_c,
       avgIf(value, metric_name='node_hwmon_temp_celsius'
             AND attributes['chip'] = ? AND attributes['sensor']='temp2')    AS gpu_temp_junction_c,
       avgIf(value, metric_name='node_hwmon_temp_celsius'
             AND attributes['chip'] = ? AND attributes['sensor']='temp3')    AS gpu_temp_mem_c,
       avgIf(value, metric_name IN ('node_hwmon_power_average_watt',
             'node_hwmon_power_average_watts') AND attributes['chip'] = ?)   AS gpu_power_w
FROM otel_metrics_gauge
WHERE server_id = ? AND ts >= now() - INTERVAL ? HOUR
GROUP BY ts ORDER BY ts
```

`toStartOfInterval` returns `DateTime` (not `DateTime64`) → use `clickhouse::serde::time::datetime`.
`0.0` from `avgIf` with no data → converted to `None` in response.

---

## Thermal State Machine

**File**: `crates/veronex/src/infrastructure/outbound/capacity/thermal.rs`

5-state machine per provider, driven by `temp_c` from node-exporter (junction temp preferred for AMD):

```
Normal ──(≥ soft_at)──→ Soft ──(≥ hard_at)──→ Hard
  ↑                       ↑                      │
  │                       │                      │ (< normal_below)
  │                       │                      ↓
  │                       │                   Cooldown (60s hold)
  │                       │                      │
  │                       │                      ↓
  └───────────────────────┴──────────────── RampUp (max_concurrent=1)
                                               │ (AIMD restores → pre_hard level)
                                               ↓
                                            Normal
```

| State | Condition | Dispatch Effect |
|-------|-----------|-----------------|
| Normal | `temp < normal_below` | Full capacity |
| Soft | `temp ≥ soft_at` | Block if provider has ANY active request |
| Hard | `temp ≥ hard_at` | Block ALL requests |
| Cooldown | Hard → `temp < normal_below`, 60s timer | Block ALL (cooling hold) |
| RampUp | After Cooldown expires | `max_concurrent` forced to 1, AIMD gradually increases |

### Threshold Profiles (auto-detected from gpu_vendor)

| Profile | normal_below | soft_at | hard_at | Detection |
|---------|-------------|---------|---------|-----------|
| CPU (default) | 75°C | 82°C | 90°C | AMD (`0x1002`) or unknown |
| GPU | 80°C | 88°C | 93°C | NVIDIA (`0x10de`) |

### perf_factor (Temperature-Proportional)

```
perf_factor(temp_c) → 0.0 to 1.0
  75°C → 1.0 (full performance)
  90°C → 0.0 (zero performance)
  Linear interpolation between thresholds
```

Used in ZSET scoring: `age_bonus = wait_secs × perf_factor(temp_c)` — hot servers get deprioritized for new work.

### Admin API

Thresholds are set automatically by `health_checker` every 30s cycle based on `gpu_vendor`. Custom thresholds can be set programmatically:

```rust
thermal.set_thresholds(provider_id, ThermalThresholds::GPU);
thermal.set_thresholds(provider_id, ThermalThresholds {
    normal_below: 70.0,
    soft_at: 80.0,
    hard_at: 88.0,
});
```

---

## Web UI

→ See `docs/llm/frontend/pages/servers.md` → ServersTab
