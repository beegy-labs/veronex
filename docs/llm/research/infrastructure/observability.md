# Observability (OTel) — 2026 Research

> **Last Researched**: 2026-03-01 | **Source**: OTel docs + verified in production
> **Status**: Verified — used in docker-compose + `crates/veronex-analytics/`

---

## Architecture: veronex OTel Pipeline

```
veronex API
  ├── HttpObservabilityAdapter → POST /internal/ingest/inference  (veronex-analytics)
  └── HttpAuditAdapter         → POST /internal/ingest/audit       (veronex-analytics)
            ↓
  veronex-analytics (port 3003)
    └── OTel LogRecord → OTLP gRPC → OTel Collector
            ↓
  OTel Collector (config.yaml)
    ├── receivers:  prometheus, otlp
    └── exporters:
        ├── kafka/metrics  → Redpanda [otel-metrics]  → MV → otel_metrics_gauge (MergeTree)
        ├── kafka/traces   → Redpanda [otel-traces]   → otel_traces_raw (MergeTree)
        └── kafka/logs     → Redpanda [otel-logs]     → MV → otel_logs (MergeTree)
```

**Key decision**: veronex crate has **no rskafka, no clickhouse** deps.
All analytics write goes through veronex-analytics HTTP API. Fail-open: HTTP errors → `warn!`, inference continues.

---

## ClickHouse — Kafka Engine + MV Pattern

```sql
-- 1. Kafka Engine table (reads from Redpanda topic)
CREATE TABLE kafka_otel_logs (
  RawMessage String
) ENGINE = Kafka('redpanda:9092', 'otel-logs', 'ch-consumer', 'JSONAsString');

-- 2. Target MergeTree table
CREATE TABLE otel_logs (
  Timestamp DateTime64(9),
  LogAttributes Map(String, String),
  Body String,
  -- ...
) ENGINE = MergeTree() ORDER BY Timestamp;

-- 3. Materialized View: Kafka → MergeTree (with arrayJoin for nested arrays)
CREATE MATERIALIZED VIEW kafka_otel_logs_mv TO otel_logs AS
SELECT
  arrayJoin(arrayJoin(arrayJoin(
    JSONExtract(RawMessage, 'resourceLogs', 'Array(Array(Array(String)))')
  ))) ...
FROM kafka_otel_logs;
```

**Note**: `otel_logs` MV uses 3-level `arrayJoin` (resourceLogs → scopeLogs → logRecords).

---

## OTel Collector Config — Receivers

```yaml
receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: 'node-exporter'
          static_configs:
            - targets: ['host.docker.internal:9100']
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318
```

---

## Metrics Schema (otel_metrics_gauge)

Used by `GET /v1/servers/{id}/metrics/history?hours=N`.

```sql
-- Key metric names for GPU server history
node_memory_MemTotal_bytes
node_memory_MemAvailable_bytes
node_hwmon_temp_celsius        -- GPU temp (chip label = amdgpu PCI addr)
node_hwmon_power_average_watt  -- GPU power (also: node_hwmon_power_average_watts)
```

ClickHouse `toStartOfInterval` returns `DateTime` (not `DateTime64`) →
use `clickhouse::serde::time::datetime` deserializer, not `datetime64`.

---

## Logs Schema (otel_logs)

Unified event store. Discriminated by `LogAttributes['event.name']`.

| `event.name` | Payload | Writer |
|-------------|---------|--------|
| `inference.completed` | model, tokens, latency, key_id, provider | HttpObservabilityAdapter |
| `audit.action` | action, resource_type, resource_id, account_id, ip | HttpAuditAdapter |

---

## Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| ClickHouse exporter in OTel Collector | Couples infra, hard to scale | Kafka Engine + MV pattern |
| Sync Kafka write in request path | Adds latency + failure risk | HTTP to veronex-analytics (fail-open) |
| Direct ClickHouse write from app | Tight coupling | Route through veronex-analytics service |
| Using `DateTime64` for `toStartOfInterval` result | Type mismatch in Rust deserializer | Use `datetime` serde, not `datetime64` |

---

## Sources

- OTel Collector docs: https://opentelemetry.io/docs/collector/
- ClickHouse Kafka Engine: https://clickhouse.com/docs/engines/table-engines/integrations/kafka
- Verified: `docker/otel/config.yaml`, `docker/clickhouse/init.sql`, `crates/veronex-analytics/`
