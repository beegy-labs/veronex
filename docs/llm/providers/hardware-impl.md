# Hardware: node-exporter, GPU Vendor & Service Health

> SSOT | **Last Updated**: 2026-03-21
> Core entity, DB schema, API endpoints, metric structs: `providers/hardware.md`

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

---

## GPU Vendor Detection via node-exporter

GPU vendor is inferred from **DRM metric presence** — amdgpu kernel driver exposes DRM metrics via node-exporter; NVIDIA uses proprietary driver and does not:

| Condition | `gpu_vendor` | Thermal Profile |
|-----------|-------------|-----------------|
| DRM GPU metrics present (`node_drm_*`) | `"amd"` | CPU (75/82/90°C) — covers AMD discrete + Ryzen AI APU |
| No DRM GPU metrics | `""` (empty) | CPU (default) |

The `gpu_vendor` field is set by `run_server_metrics_loop` — an independent background loop that iterates `gpu_servers` directly (not via providers). This loop caches `NodeMetrics` in Valkey per server and persists `gpu_vendor` to DB. Server liveness is decoupled from provider routing state, so the Servers page shows live metrics even when no provider is linked.

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
| `domain/constants.rs` | `service_health_key(instance_id)` canonical key + `SERVICE_HEALTH_TTL_SECS` |
| `valkey_keys.rs` | `service_health(instance_id)` pk-aware shim for direct-fred callers |
