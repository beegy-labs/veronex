# OTel Pipeline: Collector Config & Chains

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> OTel Collector config, processing chains, derived materialized views, and PG fallback pattern.

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

> ClickHouse exporter **removed** -- Chain 1 (logs) uses Kafka Engine; Chains 2-3 (metrics/traces) use Redpanda Connect HTTP INSERT.
> `otlp` receiver is shared by all three pipelines (metrics, traces, logs).
> No `prometheus` receiver — agent handles external node-exporter scraping (supports bare-metal outside K8s).

---

## Chain 1 -- otel-logs -> otel_logs

Produced by `veronex-analytics` via OTel Logs SDK -> OTel Collector -> Redpanda `otel-logs`.
One Kafka Engine chain (Chain 1 only). Chains 2 and 3 use Redpanda Connect HTTP INSERT — see `otel-pipeline-ops.md`.

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
