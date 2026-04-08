# Agent Data Optimization SDD

> **Status**: Planned | **Last Updated**: 2026-03-15
> **Branch**: feat/ollama-compat-non-streaming (same feature branch)

---

## Goal

Minimize storage volume in the Agent → OTel Collector → Redpanda → ClickHouse pipeline.

---

## Current State (measured 2026-03-15)

### Redpanda (50G PVC, 3.9G used)

| Topic | Size | Cause |
|-------|------|------|
| `otel.audit.metrics` | 2.1G | no retention → unbounded accumulation |
| `otel.audit.logs` | 1.2G | no retention → unbounded accumulation |
| `otel.audit.traces` | 97M | no retention |

> Kafka/Redpanda does not delete messages even after consumer consumption without retention settings.
> ClickHouse consumes near real-time but messages remain.

### ClickHouse (veronex DB)

| Table | Row count | Size | Hourly ingestion |
|--------|-------|------|------------|
| `otel_metrics_gauge` | 16.3M | 121 MiB | 806,420 rows |
| `otel_logs` | 1.0M | 70.6 MiB | 2,714 rows |
| `audit_events` | 9 | 2.16 KiB | — |

> TTL set as placeholder in init migration → substituted at deploy via Helm values: metrics 30d, analytics 90d, audit 365d.

### Agent Collected Metrics Analysis (hourly, top)

| Metric | rows/hr | Share | Necessity |
|--------|---------|------|--------|
| `node_cpu_seconds_total` | 187,460 | 23% | Partial — only user/system/iowait needed |
| `node_network_*` (25 types) | ~250,000 | 31% | Unnecessary — not in current allowlist but historical data exists in ClickHouse |
| `node_cpu_scaling_governor` | 31,633 | 4% | Unnecessary |
| `node_cpu_guest_seconds_total` | 31,633 | 4% | Unnecessary |
| `node_cpu_scaling_frequency_*` (4 types) | 63,000 | 8% | Unnecessary |
| `node_hwmon_chip_names` | 4,066 | 0.5% | Unnecessary (static label) |
| `node_hwmon_sensor_label` | 6,226 | 0.8% | Unnecessary (static label) |
| `node_drm_*`, `node_hwmon_temp_*` | ~15,000 | 2% | Required (core GPU monitoring) |
| `node_memory_*`, `ollama_*` | ~8,000 | 1% | Required |

---

## Optimization Plan

### Phase 1 — Redpanda Retention Configuration (highest impact)

**Target**: platform-gitops `clusters/home/values/redpanda-values.yaml`

Set retention on all otel topics:

| Topic | Retention | Rationale |
|-------|-----------|------|
| `otel.audit.metrics` | 2 hours | ClickHouse consumes real-time, only buffer headroom needed |
| `otel.audit.logs` | 2 hours | Same |
| `otel.audit.traces` | 2 hours | Same |

**Expected impact**: 3.4G → ~200MB (98% reduction)

### Phase 2 — Agent Allowlist Refinement

**Target**: `crates/veronex-agent/src/scraper.rs`

Remove/modify from current `NODE_EXPORTER_ALLOWLIST`:

| Change | Item | Reason |
|------|------|------|
| Remove | `node_cpu_scaling_governor` | Clock governor — analysis unnecessary |
| Remove | `node_cpu_guest_seconds_total` | VM guest CPU — unnecessary for bare metal |
| Remove | `node_hwmon_chip_names` | static label, value meaningless |
| Remove | `node_hwmon_sensor_label` | static label, value meaningless |
| Keep | `node_cpu_seconds_total` | Keep all modes (mode filter in OTel) |
| Keep | `node_drm_*` | Core GPU monitoring |
| Keep | `node_hwmon_temp_celsius` | Core thermal protection |
| Keep | `node_hwmon_power_average_watt` | Power monitoring |
| Keep | `node_memory_*` | Core memory monitoring |
| Keep | `ollama_*` | Core Ollama status |

**Expected impact**: ~12% reduction in metrics collection (67K rows/hr reduction)

### Phase 3 — ClickHouse TTL Addition

**Target**: `migrations/clickhouse/000001_init.up.sql` + 4 SQL files

```sql
-- otel_metrics_gauge
ALTER TABLE veronex.otel_metrics_gauge
  MODIFY TTL toDateTime(ts) + INTERVAL 30 DAY;

-- otel_logs
ALTER TABLE veronex.otel_logs
  MODIFY TTL toDateTime(Timestamp) + INTERVAL 7 DAY;
```

| Table | TTL | Rationale |
|--------|-----|------|
| `otel_metrics_gauge` | 30 days | Monthly trend analysis needed |
| `otel_logs` | 7 days | Only recent logs useful for debugging |

**Expected impact**: prevents unbounded long-term growth, current data starts cleanup after 30 days

### Phase 4 — Scrape Interval Adjustment (optional)

**Target**: `clusters/home/values/veronex-dev-values.yaml` + prod values

Current `scrapeIntervalMs: 15000` → `60000` (15s → 60s)

**Impact**: 4× metrics reduction (806K → ~200K rows/hr)
**Tradeoff**: 1-minute delay in GPU temperature monitoring real-time → irrelevant since thermal protection is agent-internal logic

---

## Implementation Order

| Order | Phase | Expected reduction | Notes |
|------|-------|----------|------|
| 1 | Redpanda Retention | Redpanda 98% | GitOps → Terraform, immediate effect |
| 2 | ClickHouse TTL | Prevent unbounded long-term growth | DB migration |
| 3 | Agent Allowlist | metrics 12% reduction | Code change + build/deploy |
| 4 | Scrape Interval | metrics 75% reduction | values.yaml change |

## Tasks

| # | Task | Target | Status |
|---|------|------|--------|
| 1 | Redpanda topic retention configuration | platform-gitops | pending |
| 2 | ClickHouse `otel_metrics_gauge` TTL 30 days | migration SQL | **done** (init migration TTL placeholder + Helm values) |
| 3 | ClickHouse `otel_logs` TTL 90 days | migration SQL | **done** (init migration TTL placeholder + Helm values) |
| 4 | Agent allowlist remove 4 items | scraper.rs | **done** (not included in allowlist) |
| 5 | Scrape interval adjusted to 60s | values.yaml | **done** |
