# Infrastructure — OTel Pipeline, Redpanda & ClickHouse

> SSOT | **Last Updated**: 2026-02-28 (rev: Redpanda-first DE pipeline)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change scrape interval | `docker/otel/config.yaml` | `scrape_interval:` under prometheus receiver |
| Add new OTel exporter | `docker/otel/config.yaml` | Add to `exporters:` block + add to relevant `service.pipelines` |
| Change Kafka topic name | `docker/otel/config.yaml` kafka exporter `topic:` | Also update ClickHouse `02_kafka.sql` Kafka Engine `kafka_topic_list` |
| Add new ClickHouse Kafka chain | `docker/clickhouse/02_kafka.sql` | Pattern: `kafka_* ENGINE=Kafka` → `MV` → target `MergeTree` (see Gotchas below) |
| Add new target MergeTree table | `docker/clickhouse/init.sql` | Create here — NOT in `02_kafka.sql`; init.sql runs unconditionally at first boot |
| Change HTTP SD endpoint auth | `infrastructure/inbound/http/gpu_server_handlers.rs` | Move route outside/inside auth middleware in `router.rs` |
| Migrate to managed Kafka | `docker/otel/config.yaml` `brokers:` + `docker-compose.yml` `REDPANDA_URL` | Address swap only — no code changes |

## Key Files

| File | Purpose |
|------|---------|
| `docker/otel/Dockerfile` | OTel Collector image (debian-wrapped, adds wget for healthcheck) |
| `docker/otel/config.yaml` | Receiver + exporter + pipeline config |
| `docker/clickhouse/init.sql` | MergeTree target tables (`inference_logs`, `otel_metrics_gauge`, `otel_traces_raw`, …) |
| `docker/clickhouse/02_kafka.sql` | Kafka Engine tables + Materialized Views (all 3 chains) |
| `docker-compose.yml` | `otel-collector`, `redpanda`, `clickhouse`, `veronex` services |
| `crates/inferq/src/infrastructure/outbound/observability/redpanda_adapter.rs` | `RedpandaObservabilityAdapter` |
| `crates/inferq/src/infrastructure/inbound/http/gpu_server_handlers.rs` | `GET /v1/metrics/targets` (HTTP SD) |

---

## Pipeline Overview

```
[Rust]          veronex ──────────────────────────────────────────────────────────────┐
                                                                                       │ produce JSON
                                                                                       ▼
[Redpanda]      ┌──────────────────┐  ┌──────────────────────┐  ┌──────────────────┐
                │ inference        │  │ otel-metrics         │  │ otel-traces      │
                │ (1 partition)    │  │ (1 partition)        │  │ (1 partition)    │
                └────────┬─────────┘  └──────────┬───────────┘  └────────┬─────────┘
                         │                        │                       │
[ClickHouse]    Kafka Engine                Kafka Engine            Kafka Engine
                kafka_inference             kafka_otel_metrics      kafka_otel_traces
                         │ MV                     │ MV (arrayJoin)        │ MV
                         ▼                        ▼                       ▼
                  inference_logs          otel_metrics_gauge        otel_traces_raw

[OTel Collector] prometheus (node-exporters) ──→ kafka/metrics (otel-metrics topic)
                 otlp (veronex traces)      ──→ kafka/traces  (otel-traces  topic)
```

- **Redpanda = single message bus** — all writes go through it
- **ClickHouse = read layer only** — Kafka Engine pulls from Redpanda, MV inserts into MergeTree
- Kafka 100% compatible: swap `kafka_broker_list` / `REDPANDA_URL` to migrate to Kafka cluster

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

service:
  pipelines:
    metrics: receivers:[prometheus]  → exporters:[kafka/metrics]
    traces:  receivers:[otlp]        → exporters:[kafka/traces]
```

> ClickHouse exporter **removed** — ClickHouse now consumes via Kafka Engine only.

---

## ClickHouse Kafka Engine Chains (02_kafka.sql)

### Chain 1 — inference → inference_logs

```sql
CREATE TABLE kafka_inference (raw String) ENGINE = Kafka SETTINGS
  kafka_broker_list='redpanda:9092', kafka_topic_list='inference',
  kafka_group_name='clickhouse-inference', kafka_format='JSONAsString';

CREATE MATERIALIZED VIEW kafka_inference_mv TO inference_logs AS
SELECT
  fromUnixTimestamp64Milli(JSONExtractInt(raw, 'event_time_ms')) AS event_time,
  toUUID(JSONExtractString(raw, 'api_key_id'))   AS api_key_id,
  JSONExtractString(raw, 'tenant_id')            AS tenant_id,
  toUUID(JSONExtractString(raw, 'request_id'))   AS request_id,
  JSONExtractString(raw, 'model_name')           AS model_name,
  JSONExtractUInt(raw, 'prompt_tokens')          AS prompt_tokens,
  JSONExtractUInt(raw, 'completion_tokens')      AS completion_tokens,
  JSONExtractUInt(raw, 'latency_ms')             AS latency_ms,
  JSONExtractString(raw, 'finish_reason')        AS finish_reason,
  JSONExtractString(raw, 'status')               AS status
FROM kafka_inference;
```

**Inference JSON schema** (produced by `RedpandaObservabilityAdapter`):
```json
{
  "event_time_ms": 1772213813950,
  "api_key_id": "019c9ff1-934e-...",
  "tenant_id": "",
  "request_id": "019ca02c-c262-...",
  "model_name": "qwen3:8b",
  "prompt_tokens": 0,
  "completion_tokens": 285,
  "latency_ms": 12168,
  "finish_reason": "stop",
  "status": "completed"
}
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

`init.sql` runs before `02_kafka.sql`. All MergeTree target tables (`inference_logs`,
`otel_metrics_gauge`, `otel_traces_raw`) **must be defined in `init.sql`**, not in `02_kafka.sql`.

### 3. Init scripts run only on first volume creation

`docker-entrypoint-initdb.d/` scripts run only when the ClickHouse data volume is first created.
On an existing volume, apply changes manually:

```bash
# Apply new Kafka Engine chains to existing ClickHouse
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --multiquery < docker/clickhouse/02_kafka.sql

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

## Rust Observability Adapter

**`infrastructure/outbound/observability/redpanda_adapter.rs`**

```rust
pub struct RedpandaObservabilityAdapter {
    partition_client: Arc<PartitionClient>,  // rskafka 0.5, partition 0
}
// Produces flat JSON to 'inference' topic on every ObservabilityPort::record_inference() call
// Fail-open: errors are logged as WARN, never propagated to caller
```

`REDPANDA_URL` env var (default `localhost:9092`; docker-compose: `redpanda:9092`).
Connection established at startup. If Redpanda is unreachable → `observability = None` (fail-open).

---

## Prometheus HTTP SD

`GET /v1/metrics/targets` — no auth, OTel Collector only:

```json
[{
  "targets": ["192.168.1.10:9100"],
  "labels": { "server_id": "uuid", "server_name": "gpu-node-1", "host": "192.168.1.10" }
}]
```

- Only servers with `node_exporter_url` set
- `host` extracted from `node_exporter_url`
- Multiple backends on same server → one target (deduped by server_id)

---

## Redpanda

```yaml
image: docker.redpanda.com/redpandadata/redpanda:v24.2.7
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

## Verification

```bash
# 1. Check all Kafka Engine + MV tables exist
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --query "SHOW TABLES" | grep -E "kafka_|_mv"

# 2. Consume raw inference events from Redpanda
docker compose exec redpanda rpk topic consume inference -n 1 | jq .

# 3. Confirm ClickHouse inference_logs populated
curl "http://localhost:8123/?query=SELECT+event_time,model_name,status,finish_reason,latency_ms+FROM+veronex.inference_logs+ORDER+BY+event_time+DESC+LIMIT+5+FORMAT+Vertical&user=veronex&password=veronex"

# 4. Confirm otel_metrics_gauge populated (after node-exporter scrape ~30s)
curl "http://localhost:8123/?query=SELECT+count()+FROM+veronex.otel_metrics_gauge&user=veronex&password=veronex"

# 5. Dashboard performance endpoint (reads from inference_logs)
curl http://localhost:3001/v1/dashboard/performance \
  -H "X-API-Key: veronex-bootstrap-admin-key"
```

---

## Helm Deployment Scenarios

```bash
# Default (all services)
helm install veronex ./helm/inferq/

# External Kafka (e.g. Confluent Cloud, MSK)
helm install veronex ./helm/inferq/ \
  --set redpanda.enabled=false \
  --set otelCollector.kafka.brokers="kafka-broker:9092" \
  --set inferq.env.REDPANDA_URL="kafka-broker:9092"

# Disable OTel Collector (use existing)
helm install veronex ./helm/inferq/ --set otelCollector.enabled=false
```

When using existing OTel Collector, add this scrape job and kafka exporters:
```yaml
receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: veronex-node-exporters
          http_sd_configs:
            - url: http://<release>.<namespace>.svc.cluster.local:3000/v1/metrics/targets
exporters:
  kafka/metrics:
    brokers: [<kafka-broker>:9092]
    topic: otel-metrics
    encoding: otlp_json
  kafka/traces:
    brokers: [<kafka-broker>:9092]
    topic: otel-traces
    encoding: otlp_json
```
