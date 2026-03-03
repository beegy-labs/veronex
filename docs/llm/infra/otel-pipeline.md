# Infrastructure — OTel Pipeline, Redpanda & ClickHouse

> SSOT | **Last Updated**: 2026-03-03 (rev: OTel Logs pipeline + veronex-analytics; kafka_inference/kafka_audit removed)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change scrape interval | `docker/otel/config.yaml` | `scrape_interval:` under prometheus receiver |
| Add new OTel exporter | `docker/otel/config.yaml` | Add to `exporters:` block + add to relevant `service.pipelines` |
| Change Kafka topic name | `docker/otel/config.yaml` kafka exporter `topic:` | Also update ClickHouse `init.sql` Kafka Engine `kafka_topic_list` |
| Add new ClickHouse Kafka chain | `docker/clickhouse/init.sql` | Pattern: `kafka_* ENGINE=Kafka` → `MV` → target `MergeTree` (MergeTree table must be declared first) |
| Add new target MergeTree table | `docker/clickhouse/init.sql` | Declare before the Kafka Engine section (top of file) |
| Change HTTP SD endpoint auth | `infrastructure/inbound/http/gpu_server_handlers.rs` | Move route outside/inside auth middleware in `router.rs` |
| Migrate to managed Kafka | `docker/otel/config.yaml` `brokers:` + `docker-compose.yml` `REDPANDA_URL` | Address swap only — no code changes |

## Key Files

| File | Purpose |
|------|---------|
| `docker/otel/Dockerfile` | OTel Collector image (debian-wrapped, adds wget for healthcheck) |
| `docker/otel/config.yaml` | Receiver + exporter + pipeline config (metrics, traces, logs) |
| `docker/clickhouse/schema.sql` | ClickHouse tables: MergeTree targets + Kafka Engine + Materialized Views |
| `docker/clickhouse/init.sh` | Init script — substitutes `__RETENTION_*__` vars into schema.sql |
| `docker-compose.yml` | `otel-collector`, `redpanda`, `clickhouse`, `veronex`, `veronex-analytics` services |
| `crates/veronex-analytics/src/` | Internal analytics service (OTel write + ClickHouse read) |
| `crates/veronex/src/infrastructure/outbound/observability/http_observability_adapter.rs` | `HttpObservabilityAdapter` (replaces RedpandaObservabilityAdapter) |
| `crates/veronex/src/infrastructure/outbound/observability/http_audit_adapter.rs` | `HttpAuditAdapter` (replaces RedpandaAuditAdapter) |
| `crates/veronex/src/infrastructure/inbound/http/gpu_server_handlers.rs` | `GET /v1/metrics/targets` (HTTP SD) |

---

## Pipeline Overview

```
[Write Path]
veronex ──→ POST /internal/ingest/inference ──┐
veronex ──→ POST /internal/ingest/audit    ──→ veronex-analytics
                                               └─ OTel Logs SDK (OTLP gRPC) ──→ OTel Collector :4317
                                                                                  └─ kafka/logs ──→ Redpanda [otel-logs]
                                                                                                    └─ kafka_otel_logs_mv ──→ otel_logs

node-exporters ──→ OTel Collector (prometheus) ──→ kafka/metrics ──→ Redpanda [otel-metrics] ──→ otel_metrics_gauge
veronex traces  ──→ OTel Collector (otlp)      ──→ kafka/traces  ──→ Redpanda [otel-traces]  ──→ otel_traces_raw

[Read Path]
veronex ──→ GET /v1/usage             ──→ analytics_repo ──→ POST/GET /internal/* (veronex-analytics)
veronex ──→ GET /v1/dashboard/*       ──→ analytics_repo ──┘  └─ ClickHouse otel_logs / otel_metrics_gauge
veronex ──→ GET /v1/audit             ──→ analytics_repo ──┘
```

**Key properties:**
- **Redpanda = single message bus** — all writes go through it
- **ClickHouse = read layer only** — Kafka Engine pulls from Redpanda, MV inserts into MergeTree
- **veronex-analytics** = internal service (port 3003, not exposed) — owns all OTel write + ClickHouse read
- **`otel_logs` = unified event store** — inference + audit events keyed by `LogAttributes['event.name']`
- **veronex crate** = no direct Redpanda or ClickHouse dependency (removed rskafka + clickhouse crates)

---

## OTel Collector Config (docker/otel/config.yaml)

```yaml
receivers:
  prometheus:               # node-exporter scrape via HTTP SD
    config:
      scrape_configs:
        - job_name: node-exporter
          scrape_interval: 30s
          http_sd_configs:
            - url: http://veronex:3000/v1/metrics/targets
              refresh_interval: 30s
  otlp:
    protocols:
      grpc: { endpoint: 0.0.0.0:4317 }
      http: { endpoint: 0.0.0.0:4318 }

exporters:
  kafka/metrics:
    brokers: [redpanda:9092]
    topic: otel-metrics
    encoding: otlp_json       # camelCase protobuf JSON (resourceMetrics, timeUnixNano, …)
  kafka/traces:
    brokers: [redpanda:9092]
    topic: otel-traces
    encoding: otlp_json
  kafka/logs:                 # NEW — inference + audit events from veronex-analytics
    brokers: [redpanda:9092]
    topic: otel-logs
    encoding: otlp_json       # OTLP JSON (resourceLogs, scopeLogs, logRecords, …)

service:
  pipelines:
    metrics: receivers:[prometheus]  → exporters:[kafka/metrics]
    traces:  receivers:[otlp]        → exporters:[kafka/traces]
    logs:    receivers:[otlp]        → exporters:[kafka/logs]   # NEW
```

> ClickHouse exporter **removed** — ClickHouse consumes via Kafka Engine only.
> `otlp` receiver is shared by both `traces` and `logs` pipelines.

---

## ClickHouse Kafka Engine Chains (init.sql)

Three active chains. Old `kafka_inference` + `kafka_audit` chains removed (superseded by `kafka_otel_logs`).

### Chain 1 — otel-logs → otel_logs

Produced by `veronex-analytics` via OTel Logs SDK → OTel Collector → Redpanda `otel-logs`.

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
    -- resource attributes → Map (stringValue | intValue | doubleValue)
    CAST(arrayMap(x -> (JSONExtractString(x,'key'), COALESCE(
        nullIf(JSONExtractString(JSONExtractRaw(x,'value'),'stringValue'),''),
        nullIf(JSONExtractString(JSONExtractRaw(x,'value'),'intValue'),''),
        toString(JSONExtractFloat(JSONExtractRaw(x,'value'),'doubleValue'))
      )), JSONExtractArrayRaw(JSONExtractRaw(rm,'resource'),'attributes')),
      'Map(LowCardinality(String), String)') AS ResourceAttributes,
    -- log record attributes (event.name, api_key_id, latency_ms, …)
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
- `event.name`: `"inference.completed"` | `"audit.action"`
- Inference: `api_key_id`, `request_id`, `model_name`, `prompt_tokens`, `completion_tokens`, `latency_ms`, `finish_reason`, `status`, `provider_type`
- Audit: `account_id`, `account_name`, `action`, `resource_type`, `resource_id`, `resource_name`

**Target table** (`docker/clickhouse/init.sql`):
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

### Chain 2 — otel-metrics → otel_metrics_gauge

OTLP JSON (camelCase) is unpacked via `arrayJoin`:

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
  JSONExtractString(metric, 'name')        AS MetricName,
  JSONExtractString(metric, 'description') AS MetricDescription,
  JSONExtractString(metric, 'unit')        AS MetricUnit,
  CAST(arrayMap(x -> (JSONExtractString(x,'key'),
    JSONExtractString(JSONExtractRaw(x,'value'),'stringValue')),
    JSONExtractArrayRaw(dp, 'attributes')),
    'Map(LowCardinality(String), String)') AS Attributes,
  fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(dp,'startTimeUnixNano'))) AS StartTimeUnix,
  fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(dp,'timeUnixNano')))      AS TimeUnix,
  JSONExtractFloat(dp, 'asDouble') AS Value,
  0 AS Flags,
  -- Nested type → flat sub-columns (see Gotcha #1)
  CAST([], 'Array(Map(LowCardinality(String), String))') AS `Exemplars.FilteredAttributes`,
  CAST([], 'Array(DateTime64(9))')                       AS `Exemplars.TimeUnix`,
  CAST([], 'Array(Float64)')                             AS `Exemplars.Value`,
  CAST([], 'Array(String)')                              AS `Exemplars.SpanId`,
  CAST([], 'Array(String)')                              AS `Exemplars.TraceId`
FROM (
  SELECT
    arrayJoin(JSONExtractArrayRaw(raw, 'resourceMetrics')) AS rm,
    arrayJoin(JSONExtractArrayRaw(rm, 'scopeMetrics'))     AS sm,
    arrayJoin(JSONExtractArrayRaw(sm, 'metrics'))          AS metric,
    arrayJoin(JSONExtractArrayRaw(JSONExtractRaw(metric,'gauge'),'dataPoints')) AS dp
  FROM kafka_otel_metrics
  WHERE JSONHas(metric, 'gauge')
);
```

### Chain 3 — otel-traces → otel_traces_raw

```sql
CREATE TABLE kafka_otel_traces (raw String) ENGINE = Kafka SETTINGS ...;

CREATE MATERIALIZED VIEW kafka_otel_traces_mv TO otel_traces_raw AS
SELECT now64(3) AS received_at, raw AS payload
FROM kafka_otel_traces;
```

---

## Gotchas

### 1. ClickHouse `Nested` type in Materialized Views

`Nested` columns in a target MergeTree table are stored internally as parallel `Array(...)` columns.
A MV `SELECT` must reference them as **`Column.SubColumn`** — not `[] AS Exemplars`:

```sql
-- ❌ Wrong — ClickHouse throws THERE_IS_NO_COLUMN
[] AS Exemplars

-- ✅ Correct — flat sub-column aliases
CAST([], 'Array(Map(LowCardinality(String), String))') AS `Exemplars.FilteredAttributes`,
CAST([], 'Array(DateTime64(9))')                       AS `Exemplars.TimeUnix`,
CAST([], 'Array(Float64)')                             AS `Exemplars.Value`,
CAST([], 'Array(String)')                              AS `Exemplars.SpanId`,
CAST([], 'Array(String)')                              AS `Exemplars.TraceId`
```

### 2. Target tables must exist before Materialized Views

All MergeTree target tables (`otel_logs`, `otel_metrics_gauge`, `otel_traces_raw`)
**must be declared before the Kafka Engine section** in `init.sql`.

### 3. Init scripts run only on first volume creation

`docker-entrypoint-initdb.d/` scripts run only when the ClickHouse data volume is first created.
On an existing volume, apply changes manually:

```bash
# Apply new Kafka Engine chains to existing ClickHouse
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --multiquery < docker/clickhouse/init.sql

# Or drop and recreate volume (loses all data)
docker compose down -v && docker compose up -d
```

### 4. OTLP JSON key casing

OTel Collector `otlp_json` encoding uses **camelCase** protobuf field names:
`resourceMetrics`, `scopeMetrics`, `dataPoints`, `timeUnixNano`, `startTimeUnixNano`, `asDouble`.
Verify against a real message before writing new MVs:

```bash
docker compose exec redpanda rpk topic consume otel-metrics -n 1
```

---

## Rust Observability Adapters

**`infrastructure/outbound/observability/http_observability_adapter.rs`**

```rust
pub struct HttpObservabilityAdapter {
    analytics_url: String,
    analytics_secret: String,
    client: reqwest::Client,
}
// POST {ANALYTICS_URL}/internal/ingest/inference on every ObservabilityPort::record_inference()
// Fail-open: HTTP errors → warn!, never propagated to caller
```

**`infrastructure/outbound/observability/http_audit_adapter.rs`**

```rust
pub struct HttpAuditAdapter {
    analytics_url: String,
    analytics_secret: String,
    client: reqwest::Client,
}
// POST {ANALYTICS_URL}/internal/ingest/audit on every AuditPort::record()
// Fail-open: HTTP errors → warn!, never propagated to caller
```

Env vars: `ANALYTICS_URL` (default `http://localhost:3003`), `ANALYTICS_SECRET`.
If `ANALYTICS_URL` not set: `observability = None`, `audit_port = None` (fail-open).

---

## Prometheus HTTP Service Discovery (OTel Collector only)

`GET /v1/metrics/targets` — no auth, consumed by OTel Collector's `prometheus` receiver only.
> Note: Prometheus itself is **not** used. This endpoint provides HTTP SD for the OTel Collector to discover node-exporter targets; metrics are stored in ClickHouse, not Prometheus.

```json
[{
  "targets": ["192.168.1.10:9100"],
  "labels": { "server_id": "uuid", "server_name": "gpu-node-1", "host": "192.168.1.10" }
}]
```

- Only servers with `node_exporter_url` set
- `host` extracted from `node_exporter_url`
- Multiple providers on same server → one target (deduped by server_id)

---

## Redpanda

```yaml
image: docker.redpanda.com/redpandadata/redpanda:v25.3.9
command:
  - redpanda start --smp=1 --memory=512M --overprovisioned
  - --kafka-addr=PLAINTEXT://0.0.0.0:9092
  - --advertise-kafka-addr=PLAINTEXT://redpanda:9092
```

- `--smp=1 --memory=512M` — dev-only low-resource config (intentional)
- `auto_create_topics_enabled: true` — topics created on first produce
- Kafka 100% compatible: swap broker address to migrate, no code changes

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
    command:
      - --collector.drm      # AMD GPU VRAM + utilization
      - --collector.hwmon    # temperature, power
      - --collector.meminfo  # system RAM
    volumes:
      - /proc:/host/proc:ro
      - /sys:/host/sys:ro
    ports: ["9100:9100"]
```

**Registration flow**:
1. `POST /v1/servers` → register GPU server with `node_exporter_url`
2. OTel Collector polls `GET /v1/metrics/targets` every 30s → auto-starts scraping

---

## veronex-analytics Service

| Property | Value |
|----------|-------|
| Port | 3003 (internal only — `expose`, not `ports`) |
| Auth | `Authorization: Bearer {ANALYTICS_SECRET}` on all endpoints |
| Write path | `POST /internal/ingest/inference` + `POST /internal/ingest/audit` → OTel LogRecord → OTLP gRPC |
| Read path | `GET /internal/usage`, `GET /internal/performance`, `GET /internal/audit`, `GET /internal/metrics/history/{id}`, `GET /internal/analytics` |

Env vars: `CLICKHOUSE_URL`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD`, `CLICKHOUSE_DB`, `OTEL_EXPORTER_OTLP_ENDPOINT`, `ANALYTICS_SECRET`.

---

## Verification

```bash
# 1. Check Kafka Engine + MV tables exist (otel-logs chain)
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --query "SHOW TABLES" | grep -E "kafka_otel_logs|otel_logs"

# 2. Consume raw OTel logs from Redpanda
docker compose exec redpanda rpk topic consume otel-logs -n 1 | jq .

# 3. Confirm otel_logs populated (after first inference)
curl "http://localhost:8123/?query=SELECT+LogAttributes['event.name'],count()+FROM+veronex.otel_logs+GROUP+BY+1&user=veronex&password=veronex"

# 4. Confirm otel_metrics_gauge populated (after node-exporter scrape ~30s)
curl "http://localhost:8123/?query=SELECT+count()+FROM+veronex.otel_metrics_gauge&user=veronex&password=veronex"

# 5. Dashboard performance endpoint (reads from otel_logs via veronex-analytics)
curl http://localhost:3001/v1/dashboard/performance \
  -H "X-API-Key: veronex-bootstrap-admin-key"

# 6. veronex-analytics health (internal — via docker exec)
docker compose exec veronex-analytics wget -qO- http://localhost:3003/health
```

---

## Data Retention

TTLs are set per table in `docker/clickhouse/schema.sql` via `__RETENTION_*__` placeholders substituted by `init.sh` at first volume creation.

| Table | Env var | Default |
|-------|---------|---------|
| `otel_logs` | `CLICKHOUSE_RETENTION_ANALYTICS_DAYS` | 90 days |
| `otel_metrics_gauge` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 30 days |
| `otel_traces_raw` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 30 days |
| `node_metrics` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 30 days |
| `audit_events` | `CLICKHOUSE_RETENTION_AUDIT_DAYS` | 365 days |

Set in `.env` before first `docker compose up -d`. For existing volumes, use `ALTER TABLE`:

```sql
ALTER TABLE otel_logs MODIFY TTL toDate(Timestamp) + INTERVAL 30 DAY;
```
