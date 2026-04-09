# OTel Pipeline: Collector Config & Chains

> SSOT | **Last Updated**: 2026-04-09 | Classification: Operational
> OTel Collector config, processing chains, derived materialized views, and PG fallback pattern.
> **ClickHouse Kafka Engine 및 Redpanda Connect 제거 — 모든 체인은 veronex-consumer가 처리.**

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

`veronex-analytics`에서 앱레이어 가공 완료 → OTel Logs SDK → OTel Collector → Redpanda `otel-logs` → **veronex-consumer** → ClickHouse HTTP INSERT.

> ClickHouse Kafka Engine(`kafka_otel_logs`) 및 관련 MV 제거. veronex-consumer가 직접 파싱 + INSERT 담당.

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

**veronex-consumer 처리**:
- Redpanda `otel-logs` topic consume
- OTLP JSON 파싱 (resourceLogs → scopeLogs → logRecords flatten)
- `LogAttributes['event.name']` 기준으로 `inference_logs` / `audit_events` 분기 INSERT
- ClickHouse HTTP INSERT (batch)

**Log attribute keys** (veronex-analytics가 가공하여 전송, via `LogAttributes['key']`):

| event.name | Attribute keys |
|------------|----------------|
| `inference.completed` | `api_key_id`, `request_id`, `model_name`, `prompt_tokens`, `completion_tokens`, `latency_ms`, `finish_reason`, `status`, `provider_type` |
| `audit.action` | `account_id`, `account_name`, `action`, `resource_type`, `resource_id`, `resource_name` |

---

## Derived Inserts -- otel_logs 경유 specialized tables

MV 체인 제거. veronex-consumer가 `event.name` 기준으로 직접 분기 INSERT.

### veronex-consumer 분기 로직

```
otel-logs topic consume
  ├─ event.name = 'inference.completed' → INSERT INTO inference_logs
  │                                         └─ api_key_usage_hourly_mv (MergeTree MV, ClickHouse 내부) → api_key_usage_hourly
  └─ event.name = 'audit.action'        → INSERT INTO audit_events
```

> `api_key_usage_hourly_mv`는 ClickHouse 내부 집계 MV (source = `inference_logs`)로 유지. 외부 consumer가 개입하지 않음.

**Backfill**:

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
