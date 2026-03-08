# Infrastructure -- OTel Pipeline Operations

> SSOT | **Last Updated**: 2026-03-04 (rev: split from `otel-pipeline.md`)

See `infra/otel-pipeline.md` for pipeline overview, OTel Collector config, and Chain 1 (otel-logs).

---

## Chain 2 -- otel-metrics -> otel_metrics_gauge

OTLP JSON (camelCase) unpacked via `arrayJoin`. Note `Exemplars.*` flat sub-columns (see Gotchas).

```sql
CREATE TABLE kafka_otel_metrics (raw String) ENGINE = Kafka SETTINGS
  kafka_broker_list='redpanda:9092', kafka_topic_list='otel-metrics',
  kafka_group_name='clickhouse-otel-metrics', kafka_format='JSONAsString';

CREATE MATERIALIZED VIEW kafka_otel_metrics_mv TO otel_metrics_gauge AS
SELECT
  CAST(arrayMap(x -> (JSONExtractString(x,'key'),
    JSONExtractString(JSONExtractRaw(x,'value'),'stringValue')),
    JSONExtractArrayRaw(rm, 'resource.attributes')),
    'Map(LowCardinality(String), String)') AS ResourceAttributes,
  '' AS ResourceSchemaUrl, '' AS ScopeName, '' AS ScopeVersion,
  CAST(map(), 'Map(LowCardinality(String), String)') AS ScopeAttributes,
  0 AS ScopeDroppedAttrCount, '' AS ScopeSchemaUrl, '' AS ServiceName,
  JSONExtractString(metric, 'name') AS MetricName,
  JSONExtractString(metric, 'description') AS MetricDescription,
  JSONExtractString(metric, 'unit') AS MetricUnit,
  CAST(arrayMap(x -> (JSONExtractString(x,'key'),
    JSONExtractString(JSONExtractRaw(x,'value'),'stringValue')),
    JSONExtractArrayRaw(dp, 'attributes')),
    'Map(LowCardinality(String), String)') AS Attributes,
  fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(dp,'startTimeUnixNano'))) AS StartTimeUnix,
  fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(dp,'timeUnixNano'))) AS TimeUnix,
  JSONExtractFloat(dp, 'asDouble') AS Value, 0 AS Flags,
  CAST([], 'Array(Map(LowCardinality(String), String))') AS `Exemplars.FilteredAttributes`,
  CAST([], 'Array(DateTime64(9))') AS `Exemplars.TimeUnix`,
  CAST([], 'Array(Float64)') AS `Exemplars.Value`,
  CAST([], 'Array(String)') AS `Exemplars.SpanId`,
  CAST([], 'Array(String)') AS `Exemplars.TraceId`
FROM (
  SELECT
    arrayJoin(JSONExtractArrayRaw(raw, 'resourceMetrics')) AS rm,
    arrayJoin(JSONExtractArrayRaw(rm, 'scopeMetrics')) AS sm,
    arrayJoin(JSONExtractArrayRaw(sm, 'metrics')) AS metric,
    arrayJoin(JSONExtractArrayRaw(JSONExtractRaw(metric,'gauge'),'dataPoints')) AS dp
  FROM kafka_otel_metrics
  WHERE JSONHas(metric, 'gauge')
);
```

## Chain 3 -- otel-traces -> otel_traces_raw

```sql
CREATE TABLE kafka_otel_traces (raw String) ENGINE = Kafka SETTINGS ...;

CREATE MATERIALIZED VIEW kafka_otel_traces_mv TO otel_traces_raw AS
SELECT now64(3) AS received_at, raw AS payload FROM kafka_otel_traces;
```

---

## Gotchas

### ClickHouse Nested type in Materialized Views

`Nested` columns are stored as parallel `Array(...)` columns internally. A MV `SELECT` must alias each sub-column as `Column.SubColumn` individually. Using `[] AS Exemplars` causes `THERE_IS_NO_COLUMN`. See Chain 2 SQL for the correct pattern with `Exemplars.FilteredAttributes`, `Exemplars.TimeUnix`, etc.

### Target tables must exist before Materialized Views

All MergeTree targets (`otel_logs`, `otel_metrics_gauge`, `otel_traces_raw`) must be declared before the Kafka Engine section in `schema.sql`.

### Init scripts run only on first volume creation

`docker-entrypoint-initdb.d/` runs once on first volume creation. For existing volumes:

```bash
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --multiquery < docker/clickhouse/init.sql
# Or: docker compose down -v && docker compose up -d  (loses all data)
```

### OTLP JSON key casing

`otlp_json` encoding uses **camelCase** protobuf names: `resourceMetrics`, `scopeMetrics`, `dataPoints`, `timeUnixNano`, `startTimeUnixNano`, `asDouble`. Verify with `rpk topic consume otel-metrics -n 1` before writing new MVs.

---

## Rust Observability Adapters

| Adapter | File | Port method | HTTP endpoint |
|---------|------|-------------|---------------|
| `HttpObservabilityAdapter` | `http_observability_adapter.rs` | `ObservabilityPort::record_inference()` | `POST {ANALYTICS_URL}/internal/ingest/inference` |
| `HttpAuditAdapter` | `http_audit_adapter.rs` | `AuditPort::record()` | `POST {ANALYTICS_URL}/internal/ingest/audit` |

Both are fail-open: HTTP errors log `warn!`, never propagated to caller.

Env vars: `ANALYTICS_URL` (default `http://localhost:3003`), `ANALYTICS_SECRET`.
If `ANALYTICS_URL` not set: `observability = None`, `audit_port = None`.

---

## Prometheus HTTP Service Discovery

`GET /v1/metrics/targets` -- no auth, consumed by OTel Collector's `prometheus` receiver only. Prometheus itself is **not** used; metrics are stored in ClickHouse.

```json
[{ "targets": ["192.168.1.10:9100"],
   "labels": { "server_id": "uuid", "server_name": "gpu-node-1", "host": "192.168.1.10" } }]
```

Only servers with `node_exporter_url`. Multiple providers on same server = one target (deduped by server_id).

---

## Redpanda

| Property | Value |
|----------|-------|
| Image | `docker.redpanda.com/redpandadata/redpanda:v25.3.9` |
| Resources | `--smp=1 --memory=512M` (dev-only, intentional) |
| Topics | `auto_create_topics_enabled: true` |
| Migration | Swap `brokers:` address to managed Kafka -- no code changes |

---

## GPU Server Side (docker-compose.ollama.yml)

Run on each Ollama GPU server separately:

```yaml
services:
  ollama:
    image: ollama/ollama
    ports: ["11434:11434"]
  node-exporter:
    image: prom/node-exporter:latest
    command: [--collector.drm, --collector.hwmon, --collector.meminfo]
    volumes: ["/proc:/host/proc:ro", "/sys:/host/sys:ro"]
    ports: ["9100:9100"]
```

Registration: `POST /v1/servers` with `node_exporter_url` -> OTel Collector polls `/v1/metrics/targets` every 30s.

---

## veronex-analytics Service

| Property | Value |
|----------|-------|
| Port | 3003 (internal only -- `expose`, not `ports`) |
| Auth | `Authorization: Bearer {ANALYTICS_SECRET}` |
| Write | `POST /internal/ingest/inference`, `POST /internal/ingest/audit` -> OTel LogRecord -> OTLP gRPC |
| Read | `GET /internal/usage`, `/performance`, `/audit`, `/metrics/history/{id}`, `/analytics` |

Env vars: `CLICKHOUSE_URL`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD`, `CLICKHOUSE_DB`, `OTEL_EXPORTER_OTLP_ENDPOINT`, `ANALYTICS_SECRET`.

---

## Verification

```bash
# Check Kafka Engine + MV tables exist
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --query "SHOW TABLES" | grep -E "kafka_otel|otel_"

# Consume raw OTel logs from Redpanda
docker compose exec redpanda rpk topic consume otel-logs -n 1 | jq .

# Confirm otel_logs populated (after first inference)
curl "http://localhost:8123/?query=SELECT+LogAttributes['event.name'],count()+FROM+veronex.otel_logs+GROUP+BY+1&user=veronex&password=veronex"

# Confirm otel_metrics_gauge populated (after ~30s scrape)
curl "http://localhost:8123/?query=SELECT+count()+FROM+veronex.otel_metrics_gauge&user=veronex&password=veronex"

# veronex-analytics health
docker compose exec veronex-analytics wget -qO- http://localhost:3003/health
```

---

## Data Retention

TTLs set via `__RETENTION_*__` placeholders in `schema.sql`, substituted by `init.sh`.

| Table | Env var | Default |
|-------|---------|---------|
| `otel_logs` | `CLICKHOUSE_RETENTION_ANALYTICS_DAYS` | 90 days |
| `otel_metrics_gauge` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 30 days |
| `otel_traces_raw` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 30 days |
| `node_metrics` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 30 days |
| `audit_events` | `CLICKHOUSE_RETENTION_AUDIT_DAYS` | 365 days |

Set in `.env` before first `docker compose up -d`. For existing volumes, use `ALTER TABLE ... MODIFY TTL toDate(Timestamp) + INTERVAL 30 DAY`.
