# Infrastructure -- OTel Pipeline Operations

> SSOT | **Last Updated**: 2026-04-09

See `infra/otel-pipeline.md` for pipeline overview, OTel Collector config, and Chain 1 (otel-logs).

---

## Chain 2 -- otel-metrics -> otel_metrics_gauge

Lean schema — only 5 columns. `server_id` extracted from OTLP resource attributes for direct column filtering.

Handles **both** OTLP metric types:
- **gauge** — instantaneous values (memory, temperature, power)
- **sum** (`isMonotonic: true`) — monotonic counters (e.g., `node_cpu_seconds_total`)

Agent classifies metric type in `scraper.rs`; **veronex-consumer**가 두 타입 모두 처리하여 INSERT.

> Redpanda Connect 제거 — veronex-consumer가 `otel-metrics` topic consume → ClickHouse HTTP INSERT.

```sql
CREATE TABLE otel_metrics_gauge (
    ts           DateTime64(9),
    server_id    LowCardinality(String),
    metric_name  LowCardinality(String),
    value        Float64,
    attributes   Map(LowCardinality(String), String)
) ENGINE = MergeTree() PARTITION BY toDate(ts)
ORDER BY (metric_name, server_id, ts)
TTL toDate(ts) + INTERVAL __RETENTION_METRICS_DAYS__ DAY;
-- No Kafka Engine, No Redpanda Connect — veronex-consumer HTTP INSERTs directly
```

## Chain 3 -- otel-traces -> otel_traces_raw

**veronex-consumer**가 `otel-traces` topic consume → raw payload HTTP INSERT:

> Redpanda Connect 제거 — veronex-consumer가 담당.

```sql
CREATE TABLE otel_traces_raw (
    received_at DateTime64(3) DEFAULT now64(3),
    payload     String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(received_at)
ORDER BY received_at
TTL toDate(received_at) + INTERVAL __RETENTION_TRACES_DAYS__ DAY;
-- No Kafka Engine, No Redpanda Connect — veronex-consumer HTTP INSERTs directly
```

---

## Gotchas

### Target tables must exist before consumer starts

All MergeTree targets (`otel_logs`, `otel_metrics_gauge`, `otel_traces_raw`, `inference_logs`, `audit_events`) must exist before `veronex-consumer` starts. schema.sql 적용 후 consumer 기동.

### Init scripts run only on first volume creation

`docker-entrypoint-initdb.d/` runs once on first volume creation. For existing volumes:

```bash
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --multiquery < docker/clickhouse/schema.sql
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

## Metrics Target Discovery

`GET /v1/metrics/targets` -- no auth, consumed by veronex-agent (StatefulSet, modulus-sharded replicas). Returns two independent target types, each collected separately. Targets are `host[:port]` only (URL normalization strips scheme/path/query).

```json
[
  { "targets": ["192.168.1.10:9100"],
    "labels": { "type": "server", "server_id": "uuid", "server_name": "gpu-node-1" } },
  { "targets": ["192.168.1.10:11434"],
    "labels": { "type": "ollama", "provider_id": "uuid", "provider_name": "gpu-1", "server_id": "uuid" } }
]
```

- `type=server` — one per `gpu_servers` row with `node_exporter_url`, shard key = `server_id`
- `type=ollama` — one per active Ollama provider, shard key = `provider_id`, includes `server_id` when linked

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

Registration: `POST /v1/servers` with `node_exporter_url` -> veronex-agent polls `/v1/metrics/targets` each scrape cycle (default 15s).

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

## veronex-consumer Resource Bounds

`available_parallelism()` reads host CPU and ignores cgroup limits — on a 16-core node this allocated 16 worker + 128 blocking threads regardless of the pod's 500m CPU. Combined with librdkafka defaults (1 GiB-per-partition prefetch queue × 9 partitions for 3 audit topics), startup OOM-killed at any reasonable container limit. The crate now caps both axes at the source.

| Knob | Value | Why |
|------|-------|-----|
| `tokio worker_threads` | 2 | matches 500m CPU (cgroup-aware sizing) |
| `tokio max_blocking_threads` | 8 | I/O-bound INSERT path — small fixed pool |
| `queued.max.messages.kbytes` | 4096 (4 MiB) | per-partition prefetch cap (default 1 GiB) |
| `queued.min.messages` | 1000 | smaller prefetch target (default 100k) |
| `fetch.message.max.bytes` | 524288 (512 KiB) | single-message cap |
| `fetch.max.bytes` | 4 194 304 (4 MiB) | per-fetch response cap |
| `receive.message.max.bytes` | 8 388 608 (8 MiB) | protocol payload cap (≥ fetch) |

Worst-case memory budget at 9 partitions × 4 MiB queue + tokio stacks + ClickHouse pool + `MAX_BATCH=500` buffers ≈ **~110 MiB**. Helm `consumer.resources.limits.memory` should be ~256Mi (2× safety margin). Audit messages are metadata-only (≤ ~50 KiB observed) so the 512 KiB single-message cap leaves a 10× margin — large prompt/response payloads do not flow through OTel topics (they go to S3 `veronex-messages` and Postgres `conversations`).

---

## Verification

```bash
# Check target tables exist
docker compose exec clickhouse clickhouse-client \
  --user veronex --password veronex --database veronex \
  --query "SHOW TABLES" | grep -E "otel_|inference_logs|audit_events"

# Consume raw OTel logs from Redpanda (consumer 이전 단계 확인)
docker compose exec redpanda rpk topic consume otel-logs -n 1 | jq .

# Confirm otel_logs populated (veronex-consumer가 INSERT한 이후)
curl "http://localhost:8123/?query=SELECT+LogAttributes['event.name'],count()+FROM+veronex.otel_logs+GROUP+BY+1&user=veronex&password=veronex"

# Confirm otel_metrics_gauge populated (after ~30s scrape)
curl "http://localhost:8123/?query=SELECT+count()+FROM+veronex.otel_metrics_gauge&user=veronex&password=veronex"

# veronex-analytics health
docker compose exec veronex-analytics wget -qO- http://localhost:3003/health

# veronex-consumer lag (consumer group offset vs latest)
docker compose exec redpanda rpk group describe veronex-consumer
```

---

## Data Retention

TTLs set via `__RETENTION_*__` placeholders in `schema.sql`, substituted by `init.sh`.

| Table | Env var | Default |
|-------|---------|---------|
| `inference_logs` | `CLICKHOUSE_RETENTION_INFERENCE_DAYS` | 90 days |
| `otel_logs` | `CLICKHOUSE_RETENTION_LOGS_DAYS` | 7 days |
| `otel_metrics_gauge` | `CLICKHOUSE_RETENTION_METRICS_DAYS` | 14 days |
| `otel_traces_raw` | `CLICKHOUSE_RETENTION_TRACES_DAYS` | 7 days |
| `audit_events` | `CLICKHOUSE_RETENTION_AUDIT_DAYS` | 90 days |
| `mcp_tool_calls` | `CLICKHOUSE_RETENTION_MCP_DAYS` | 90 days |

Set in `.env` before first `docker compose up -d`. For existing volumes, use `ALTER TABLE ... MODIFY TTL toDate(Timestamp) + INTERVAL N DAY`.
