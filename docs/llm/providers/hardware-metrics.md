# Hardware: Metrics & Thermal

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> History buckets, ClickHouse queries, thermal state machine, and web UI.

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

-- CTE: compute CPU usage % from node_cpu_seconds_total counter deltas
WITH cpu_pct AS (
    SELECT toStartOfInterval(ts, INTERVAL {bucket_interval}) AS bucket,
           -- neighbor() window function computes delta between consecutive counter values
           -- per-CPU idle delta / total delta across all modes → idle ratio → 100 - idle = usage%
           ...  -- aggregated across all CPU cores per bucket
    FROM otel_metrics_gauge
    WHERE server_id = ? AND metric_name = 'node_cpu_seconds_total'
      AND ts >= now() - INTERVAL ? HOUR
    GROUP BY bucket
)
SELECT toStartOfInterval(g.ts, INTERVAL {bucket_interval}) AS ts,
       maxIf(g.value, g.metric_name='node_memory_MemTotal_bytes') / 1048576      AS mem_total_mb,
       avgIf(g.value, g.metric_name='node_memory_MemAvailable_bytes') / 1048576  AS mem_avail_mb,
       c.cpu_usage_pct,
       avgIf(g.value, g.metric_name='node_hwmon_temp_celsius'
             AND g.attributes['chip'] = ? AND g.attributes['sensor']='temp1')    AS gpu_temp_c,
       avgIf(g.value, g.metric_name='node_hwmon_temp_celsius'
             AND g.attributes['chip'] = ? AND g.attributes['sensor']='temp2')    AS gpu_temp_junction_c,
       avgIf(g.value, g.metric_name='node_hwmon_temp_celsius'
             AND g.attributes['chip'] = ? AND g.attributes['sensor']='temp3')    AS gpu_temp_mem_c,
       avgIf(g.value, g.metric_name IN ('node_hwmon_power_average_watt',
             'node_hwmon_power_average_watts') AND g.attributes['chip'] = ?)     AS gpu_power_w
FROM otel_metrics_gauge g
LEFT JOIN cpu_pct c ON c.bucket = toStartOfInterval(g.ts, INTERVAL {bucket_interval})
WHERE g.server_id = ? AND g.ts >= now() - INTERVAL ? HOUR
GROUP BY ts, c.cpu_usage_pct ORDER BY ts
```

**CPU usage % computation**: `node_cpu_seconds_total` is a monotonic counter (OTLP `sum` with `isMonotonic: true`). The CTE uses `neighbor()` window function to compute per-CPU deltas between consecutive scrape points, then derives `100 * (1 - idle_delta / total_delta)` aggregated across all cores.

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
  │                       │                   Cooldown (300s min, 900s max)
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
| Soft | `temp ≥ soft_at` | Block dispatch when `active_count > 0` (drain-first); exit requires `temp < normal_below AND active_count == 0` |
| Hard | `temp ≥ hard_at` | Block ALL requests |
| Cooldown | `set_cooldown()` when `active_count == 0` OR 300s elapsed (auto-fallback); requires `temp < hard_at` | Block ALL (300s min, 900s max); at 900s forced exit: `temp ≥ soft_at → Soft`, else `→ RampUp`; 90s = watchdog log only |
| RampUp | After Cooldown expires (`cooldown_elapsed` AND `temp < soft_at`) | `max_concurrent` forced to 1, AIMD gradually increases |
| RampUp → Normal | `sum_max_concurrent >= pre_hard_total` (all models restored) | Full capacity |

### Threshold Profiles (auto-detected from gpu_vendor)

| Profile | normal_below | soft_at | hard_at | Detection |
|---------|-------------|---------|---------|-----------|
| CPU (default) | 75°C | 82°C | 90°C | AMD (DRM metrics present) or unknown (no DRM) |
| GPU | 80°C | 88°C | 93°C | NVIDIA (future — currently unreachable; no DRM metrics) |

### perf_factor (Temperature-Proportional)

```
perf_factor(temp_c) → 0.0 to 1.0
  ≤75°C → 1.0  (full performance)
   82°C → 0.70 (midpoint — piecewise linear bend)
  ≥90°C → 0.0  (zero performance)

  Segment 1 (75→82°C): 1.0 → 0.70  linear
  Segment 2 (82→90°C): 0.70 → 0.0  linear
```

Used in ZSET scoring: `age_bonus = wait_ms × 0.25 × perf_factor` — hot servers get deprioritized for new work.

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
