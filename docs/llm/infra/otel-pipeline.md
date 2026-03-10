# Infrastructure -- OTel Pipeline

> SSOT | **Last Updated**: 2026-03-10 (rev: agent 2-target push, URL normalization, DoS protection)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change agent scrape interval | `values.yaml` `scrapeIntervalMs` or `docker-compose.yml` `SCRAPE_INTERVAL_MS` | Agent env var (milliseconds) |
| Add new metric to collection | `crates/veronex-agent/src/scraper.rs` | Add prefix to `NODE_EXPORTER_ALLOWLIST` |
| Add new OTel exporter | `docker/otel/config.yaml` | Add to `exporters:` block + add to relevant `service.pipelines` |
| Change Kafka topic name | `docker/otel/config.yaml` kafka exporter `topic:` | Also update ClickHouse `init.sql` Kafka Engine `kafka_topic_list` |
| Add new ClickHouse Kafka chain | `docker/clickhouse/init.sql` | Pattern: `kafka_* ENGINE=Kafka` -> `MV` -> target `MergeTree` (MergeTree table must be declared first) |
| Add new target MergeTree table | `docker/clickhouse/init.sql` | Declare before the Kafka Engine section (top of file) |
| Change HTTP SD endpoint auth | `infrastructure/inbound/http/metrics_handlers.rs` | Move route outside/inside auth middleware in `router.rs` |
| Migrate to managed Kafka | `docker/otel/config.yaml` `brokers:` + `docker-compose.yml` `REDPANDA_URL` | Address swap only -- no code changes |

## Key Files

| File | Purpose |
|------|---------|
| `docker/otel/Dockerfile` | OTel Collector image (debian-wrapped, adds wget for healthcheck) |
| `docker/otel/config.yaml` | Receiver + exporter + pipeline config (metrics, traces, logs) |
| `docker/clickhouse/schema.sql` | ClickHouse tables: MergeTree targets + Kafka Engine + Materialized Views |
| `docker/clickhouse/init.sh` | Init script -- substitutes `__RETENTION_*__` vars into schema.sql |
| `docker-compose.yml` | `otel-collector`, `redpanda`, `clickhouse`, `veronex`, `veronex-analytics` services |
| `crates/veronex-analytics/src/` | Internal analytics service (OTel write + ClickHouse read) |
| `crates/veronex/src/infrastructure/outbound/observability/http_observability_adapter.rs` | `HttpObservabilityAdapter` (replaces RedpandaObservabilityAdapter) |
| `crates/veronex/src/infrastructure/outbound/observability/http_audit_adapter.rs` | `HttpAuditAdapter` (replaces RedpandaAuditAdapter) |
| `crates/veronex/src/infrastructure/inbound/http/metrics_handlers.rs` | `GET /v1/metrics/targets` — two target types (server + ollama), URL normalization to `host[:port]` |
| `crates/veronex-agent/src/scraper.rs` | Metric allowlist + Prometheus text → OTLP conversion (raw values), body size limits (16MB node-exporter, 1MB Ollama) |
| `crates/veronex-agent/src/otlp.rs` | OTLP HTTP/JSON push client |
| `crates/veronex-agent/src/shard.rs` | Modulus sharding for multi-replica deduplication |

---

## Pipeline Overview

```
[Write Path]
veronex --> POST /internal/ingest/inference --+
veronex --> POST /internal/ingest/audit    --> veronex-analytics
                                               +- OTel Logs SDK (OTLP gRPC) --> OTel Collector :4317
                                                                                  +- kafka/logs --> Redpanda [otel-logs]
                                                                                                    +- kafka_otel_logs_mv --> otel_logs
                                                                                                         +- otel_inference_logs_mv --> inference_logs
                                                                                                         |                             +- api_key_usage_hourly_mv --> api_key_usage_hourly
                                                                                                         +- otel_audit_events_mv   --> audit_events

node-exporters (type=server) -+
                              +-> veronex-agent (select + OTLP push) --> OTel Collector (otlp) --> kafka/metrics --> Redpanda [otel-metrics] --> otel_metrics_gauge
ollama /api/ps (type=ollama) -+
veronex traces  --> OTel Collector (otlp)      --> kafka/traces  --> Redpanda [otel-traces]  --> otel_traces_raw

[Read Path — ClickHouse primary, PostgreSQL fallback]
veronex --> GET /v1/usage             --> analytics_repo (ClickHouse) --> fallback: PostgreSQL inference_jobs
veronex --> GET /v1/dashboard/*       --> analytics_repo (ClickHouse) --> fallback: PostgreSQL inference_jobs
veronex --> GET /v1/audit             --> analytics_repo (ClickHouse)
```

**Key properties:**
- **Redpanda = single message bus** -- all writes go through it
- **ClickHouse = read layer only** -- Kafka Engine pulls from Redpanda, MV inserts into MergeTree
- **veronex-analytics** = internal service (port 3003, not exposed) -- owns all OTel write + ClickHouse read
- **Timestamp semantics**: `timeUnixNano` = original event time (from veronex), `observedTimeUnixNano` = collector receipt time
- **Ingest validation**: Event type whitelist (`inference.completed`, `audit.action`), required field checks → 400 on invalid
- **`otel_logs` = unified event store** -- inference + audit events keyed by `LogAttributes['event.name']`
- **veronex crate** = no direct Redpanda or ClickHouse dependency (removed rskafka + clickhouse crates)

---

## veronex-agent Policy (Zero-Config Principle)

> **`helm install` / `docker-compose up` must work without any OTEL Collector configuration changes.**
> Agent is a pure OTLP push collector — no HTTP server, no inbound ports.

### Two Independent Target Types

Agent discovers targets via `GET /v1/metrics/targets`. Each target has a `type` label:

| Type | Source | Shard key | Collects |
|------|--------|-----------|----------|
| `server` | node-exporter `/metrics` | `server_id` | CPU, mem, GPU (DRM, hwmon) |
| `ollama` | Ollama `/api/ps` | `provider_id` | loaded models, VRAM per model |

Targets are returned as `host[:port]` only (URL normalization strips scheme/path/query). Agent prepends `http://` before scraping.

### N-Way Replication (Modulus Sharding)

StatefulSet replicas shard targets by `hash(shard_key) % replica_count == ordinal`. Ordinal is extracted from the K8s pod hostname suffix (e.g., `veronex-agent-2` → ordinal 2). Single replica (`REPLICA_COUNT=1`) owns all targets.

### DoS Protection

| Guard | Value | File |
|-------|-------|------|
| `MAX_NODE_EXPORTER_BODY` | 16 MB | `scraper.rs` |
| `MAX_OLLAMA_BODY` | 1 MB | `scraper.rs` |
| `MAX_CONCURRENT_SCRAPES` | 32 (semaphore) | `main.rs` |
| `SCRAPE_TIMEOUT` | 5 s | `scraper.rs` |

### Responsibility Split

| Responsibility | Owner | NOT allowed |
|----------------|-------|-------------|
| Metric selection (allowlist) | **Agent** (`scraper.rs`) | OTEL filter processor |
| Value transformation (unit conversion) | **ClickHouse queries** | Agent or OTEL |
| Format conversion (Prometheus text → OTLP) | **Agent** | — |
| Transport + batching | **OTEL Collector** | — |
| Routing to Kafka topics | **OTEL Collector** | — |

**Why agent selects, not OTEL:**
- Open source users should not need to configure OTEL Collector
- Adding OTEL `filter` processor requires `contrib` image (not guaranteed)
- Metric allowlist changes are code changes (version-controlled, tested)

**Allowlist** (`crates/veronex-agent/src/scraper.rs`):
```
node_memory_MemTotal_bytes, node_memory_MemAvailable_bytes,
node_cpu_seconds_total, node_drm_*, node_hwmon_temp_celsius,
node_hwmon_power_average_watt*, node_hwmon_chip_names,
ollama_* (loaded_models, model_size_vram_bytes, model_size_bytes)
```

## OTel Collector Config (docker/otel/config.yaml)

```yaml
receivers:
  otlp:                       # veronex-agent pushes pre-filtered metrics via OTLP HTTP
    protocols:
      grpc: { endpoint: 0.0.0.0:4317 }
      http: { endpoint: 0.0.0.0:4318 }

exporters:
  kafka/metrics:
    brokers: [redpanda:9092]
    topic: otel-metrics
    encoding: otlp_json       # camelCase protobuf JSON (resourceMetrics, timeUnixNano, ...)
  kafka/traces:
    brokers: [redpanda:9092]
    topic: otel-traces
    encoding: otlp_json
  kafka/logs:                 # inference + audit events from veronex-analytics
    brokers: [redpanda:9092]
    topic: otel-logs
    encoding: otlp_json       # OTLP JSON (resourceLogs, scopeLogs, logRecords, ...)

service:
  pipelines:
    metrics: receivers:[otlp]  -> exporters:[kafka/metrics]
    traces:  receivers:[otlp]  -> exporters:[kafka/traces]
    logs:    receivers:[otlp]  -> exporters:[kafka/logs]
```

> ClickHouse exporter **removed** -- ClickHouse consumes via Kafka Engine only.
> `otlp` receiver is shared by all three pipelines (metrics, traces, logs).
> No `prometheus` receiver — agent handles external node-exporter scraping (K8s 외부 bare-metal 지원).

---

## Chain 1 -- otel-logs -> otel_logs

Produced by `veronex-analytics` via OTel Logs SDK -> OTel Collector -> Redpanda `otel-logs`.
Three active Kafka Engine chains total. Chains 2 and 3 are in `otel-pipeline-ops.md`.

**Target table** (`docker/clickhouse/schema.sql`):

```sql
CREATE TABLE otel_logs (
    Timestamp          DateTime64(9),
    TraceId            String,
    SpanId             String,
    SeverityText       LowCardinality(String),
    SeverityNumber     Int32,
    ServiceName        LowCardinality(String),
    Body               String,
    ResourceAttributes Map(LowCardinality(String), String),
    LogAttributes      Map(LowCardinality(String), String)
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(Timestamp)
ORDER BY (ServiceName, Timestamp)
TTL toDate(Timestamp) + INTERVAL 90 DAY;
```

**Materialized View**:

```sql
CREATE TABLE kafka_otel_logs (raw String) ENGINE = Kafka SETTINGS
  kafka_broker_list='redpanda:9092', kafka_topic_list='otel-logs',
  kafka_group_name='clickhouse-otel-logs', kafka_format='JSONAsString';

CREATE MATERIALIZED VIEW kafka_otel_logs_mv TO otel_logs AS
SELECT
  fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(lr, 'timeUnixNano'))) AS Timestamp,
  JSONExtractString(lr, 'traceId')                    AS TraceId,
  JSONExtractString(lr, 'spanId')                     AS SpanId,
  JSONExtractString(lr, 'severityText')               AS SeverityText,
  JSONExtractInt(lr, 'severityNumber')                AS SeverityNumber,
  ResourceAttributes['service.name']                  AS ServiceName,
  JSONExtractString(JSONExtractRaw(lr, 'body'), 'stringValue') AS Body,
  ResourceAttributes,
  LogAttributes
FROM (
  SELECT lr,
    CAST(arrayMap(x -> (JSONExtractString(x,'key'), COALESCE(
        nullIf(JSONExtractString(JSONExtractRaw(x,'value'),'stringValue'),''),
        nullIf(JSONExtractString(JSONExtractRaw(x,'value'),'intValue'),''),
        toString(JSONExtractFloat(JSONExtractRaw(x,'value'),'doubleValue'))
      )), JSONExtractArrayRaw(JSONExtractRaw(rm,'resource'),'attributes')),
      'Map(LowCardinality(String), String)') AS ResourceAttributes,
    CAST(arrayMap(x -> (JSONExtractString(x,'key'), COALESCE(
        nullIf(JSONExtractString(JSONExtractRaw(x,'value'),'stringValue'),''),
        nullIf(JSONExtractString(JSONExtractRaw(x,'value'),'intValue'),''),
        toString(JSONExtractFloat(JSONExtractRaw(x,'value'),'doubleValue'))
      )), JSONExtractArrayRaw(lr,'attributes')),
      'Map(LowCardinality(String), String)') AS LogAttributes
  FROM (
    SELECT arrayJoin(JSONExtractArrayRaw(raw,'resourceLogs')) AS rm,
           arrayJoin(JSONExtractArrayRaw(rm,'scopeLogs'))     AS sl,
           arrayJoin(JSONExtractArrayRaw(sl,'logRecords'))    AS lr
    FROM kafka_otel_logs
  )
);
```

**Log attribute keys** (via `LogAttributes['key']`):

| event.name | Attribute keys |
|------------|----------------|
| `inference.completed` | `api_key_id`, `request_id`, `model_name`, `prompt_tokens`, `completion_tokens`, `latency_ms`, `finish_reason`, `status`, `provider_type` |
| `audit.action` | `account_id`, `account_name`, `action`, `resource_type`, `resource_id`, `resource_name` |

---

## Derived MVs -- otel_logs → specialized tables

Two additional Materialized Views extract structured events from `otel_logs` into domain-specific MergeTree tables for efficient analytical queries.

### otel_inference_logs_mv (otel_logs → inference_logs)

```sql
CREATE MATERIALIZED VIEW otel_inference_logs_mv TO inference_logs AS
SELECT
    Timestamp                                     AS event_time,
    toUUIDOrZero(LogAttributes['api_key_id'])     AS api_key_id,
    LogAttributes['tenant_id']                    AS tenant_id,
    toUUIDOrZero(LogAttributes['request_id'])     AS request_id,
    LogAttributes['model_name']                   AS model_name,
    toUInt32OrZero(LogAttributes['prompt_tokens']) AS prompt_tokens,
    toUInt32OrZero(LogAttributes['completion_tokens']) AS completion_tokens,
    toUInt32OrZero(LogAttributes['latency_ms'])   AS latency_ms,
    LogAttributes['finish_reason']                AS finish_reason,
    LogAttributes['status']                       AS status
FROM otel_logs
WHERE LogAttributes['event.name'] = 'inference.completed';
```

Feeds into `api_key_usage_hourly_mv` (aggregating MV on `inference_logs`).

### otel_audit_events_mv (otel_logs → audit_events)

```sql
CREATE MATERIALIZED VIEW otel_audit_events_mv TO audit_events AS
SELECT
    Timestamp                                     AS event_time,
    toUUIDOrZero(LogAttributes['account_id'])     AS account_id,
    LogAttributes['account_name']                 AS account_name,
    LogAttributes['action']                       AS action,
    LogAttributes['resource_type']                AS resource_type,
    LogAttributes['resource_id']                  AS resource_id,
    LogAttributes['resource_name']                AS resource_name,
    LogAttributes['ip_address']                   AS ip_address,
    LogAttributes['details']                      AS details
FROM otel_logs
WHERE LogAttributes['event.name'] = 'audit.action';
```

### MV Chain Summary

```
otel_logs
  ├─ otel_inference_logs_mv → inference_logs
  │                             └─ api_key_usage_hourly_mv → api_key_usage_hourly
  └─ otel_audit_events_mv  → audit_events
```

**Backfill** (after creating MVs on existing data):

```sql
INSERT INTO inference_logs SELECT ... FROM otel_logs WHERE LogAttributes['event.name'] = 'inference.completed';
INSERT INTO audit_events  SELECT ... FROM otel_logs WHERE LogAttributes['event.name'] = 'audit.action';
```

---

## PG Fallback Pattern

Usage and performance handlers use **ClickHouse primary with PostgreSQL fallback**:

1. Try `analytics_repo` (ClickHouse via veronex-analytics)
2. If result is empty (e.g. `request_count == 0`, `models.is_empty()`) or error → fall back to PostgreSQL `inference_jobs` table
3. PG fallback queries use `::float8` cast on `AVG(integer)` to avoid sqlx `numeric` decode issues

This ensures monitoring works during initial deployment (before ClickHouse pipeline has data) and degrades gracefully if the analytics pipeline is down.

---

## Cross-References

- **Chains 2-3, gotchas, verification, data retention**: `infra/otel-pipeline-ops.md`
- **Observability research**: `research/infrastructure/observability.md`
- **Deploy config**: `infra/deploy.md`
