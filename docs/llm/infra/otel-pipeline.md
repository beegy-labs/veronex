# Infrastructure -- OTel Pipeline

> SSOT | **Last Updated**: 2026-03-28 (rev: sum metric support, OTLP retry 3x backoff, node_hwmon_chip_names allowlist fix)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change agent scrape interval | `values.yaml` `scrapeIntervalMs` or `docker-compose.yml` `SCRAPE_INTERVAL_MS` | Agent env var (milliseconds) |
| Add new metric to collection | `crates/veronex-agent/src/scraper.rs` | Add prefix to `NODE_EXPORTER_ALLOWLIST` |
| Add new OTel exporter | `docker/otel/config.yaml` | Add to `exporters:` block + add to relevant `service.pipelines` |
| Change Kafka topic name | `docker/otel/config.yaml` kafka exporter `topic:` | Also update ClickHouse `schema.sql` Kafka Engine `kafka_topic_list` |
| Add new OTLP metric type | `crates/veronex-agent/src/scraper.rs` + ClickHouse MV | Agent classifies in scraper; MV must handle via `UNION ALL` |
| Add new ClickHouse chain | `docker/clickhouse/schema.sql` | Logs: Kafka Engine pattern. Metrics/traces: Redpanda Connect HTTP INSERT (no Kafka Engine) |
| Add new target MergeTree table | `docker/clickhouse/schema.sql` | Declare before the Kafka Engine section (top of file) |
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
| `crates/veronex-agent/src/otlp.rs` | OTLP HTTP/JSON push client (3 retries, exponential backoff 2s/4s/8s) |
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
                              +-> veronex-agent (select + classify + OTLP push) --> OTel Collector (otlp) --> kafka/metrics --> Redpanda [otel-metrics] --> otel_metrics_gauge (gauge + sum)
ollama /api/ps (type=ollama) -+
veronex traces  --> OTel Collector (otlp)      --> kafka/traces  --> Redpanda [otel-traces]  --> otel_traces_raw

[Read Path — ClickHouse primary, PostgreSQL fallback]
veronex --> GET /v1/usage             --> analytics_repo (ClickHouse) --> fallback: PostgreSQL inference_jobs
veronex --> GET /v1/dashboard/*       --> analytics_repo (ClickHouse) --> fallback: PostgreSQL inference_jobs
veronex --> GET /v1/audit             --> analytics_repo (ClickHouse)
```

**Key properties:**
- **Redpanda = single message bus** -- all writes go through it
- **ClickHouse = read layer only** -- Chain 1 (logs): Kafka Engine pulls from Redpanda; Chains 2-3 (metrics/traces): Redpanda Connect HTTP INSERT directly
- **veronex-analytics** = internal service (port 3003, not exposed) -- owns all OTel write + ClickHouse read
- **Timestamp semantics**: `timeUnixNano` = original event time (from veronex), `observedTimeUnixNano` = collector receipt time
- **Ingest validation**: Event type whitelist (`inference.completed`, `audit.action`), required field checks → 400 on invalid
- **`otel_logs` = unified event store** -- inference + audit events keyed by `LogAttributes['event.name']`
- **veronex crate** = no direct Redpanda or ClickHouse dependency (removed rskafka + clickhouse crates)
- **Agent is the ONLY component that does metric processing** — allowlist filtering, type classification (gauge vs sum/counter). OTel → Redpanda → ClickHouse is pure data pipeline with no transformation

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
| `OTLP_RETRIES` | 3 attempts (exponential backoff: 2s, 4s, 8s) | `otlp.rs` |

### Responsibility Split

| Responsibility | Owner | NOT allowed |
|----------------|-------|-------------|
| Metric selection (allowlist) | **Agent** (`scraper.rs`) | OTEL filter processor |
| Metric type classification (gauge vs sum) | **Agent** (`scraper.rs`) | OTEL or ClickHouse |
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
node_memory_MemTotal_bytes, node_memory_MemAvailable_bytes,       (gauge)
node_cpu_seconds_total,                                            (sum, isMonotonic: true)
node_drm_*,                                                        (gauge)
node_hwmon_temp_celsius, node_hwmon_power_average_watt*,           (gauge)
node_hwmon_chip_names,                                             (gauge — required for GPU chip→PCI address lookup)
ollama_* (loaded_models, model_size_vram_bytes, model_size_bytes)  (gauge)
```

Counter metrics (`node_cpu_seconds_total`) are sent as OTLP `sum` with `isMonotonic: true`. All other metrics are sent as OTLP `gauge`. Redpanda Connect processes both types via `UNION ALL` into `otel_metrics_gauge`.

→ Collector config, chains, MVs: `otel-pipeline-chains.md`

---

## Cross-References

- **Chains 2-3, gotchas, verification, data retention**: `infra/otel-pipeline-ops.md`
- **Observability research**: `research/infrastructure/observability.md`
- **Deploy config**: `infra/deploy.md`
