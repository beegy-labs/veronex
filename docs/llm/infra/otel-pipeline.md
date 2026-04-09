# Infrastructure -- OTel Pipeline

> SSOT | **Last Updated**: 2026-04-09 (rev: unified consumer pipeline — ClickHouse Kafka Engine + Redpanda Connect 제거, veronex-consumer 자체 구현으로 대체)

## Architecture Decision: Unified Consumer Pipeline

> **모든 파이프라인은 동일한 패턴을 따른다**: `veronex-analytics(앱레이어 가공) → OTel → Redpanda → veronex-consumer → ClickHouse`

### Why Not ClickHouse Kafka Engine (Chain 1 이전 방식)

- ClickHouse 내부 상태에 종속 — consumer group offset을 CH 내부에서 관리하므로 외부에서 lag 모니터링 불가
- MV 체인 (kafka_engine → MV → MergeTree) 은 원자적이지 않음 — MV INSERT 실패 시 offset은 이미 커밋되어 데이터 유실
- 스키마 변경 시 Kafka Engine 테이블 DROP/재생성 필요 (운영 중 변경 불가)

### Why Not Redpanda Connect (Chain 2-3 이전 방식)

- HTTP INSERT 실패 시 offset 커밋 타이밍이 불명확 — 재시도와 중복/유실 경계가 모호
- 백프레셔 제어 없음 — ClickHouse가 느릴 때 Redpanda Connect가 계속 pull하여 메모리 증가
- 별도 런타임(Redpanda Connect 바이너리) 추가 — 운영 복잡도 증가

### App-Layer Processing (veronex-analytics 책임)

ClickHouse에 도달하는 데이터는 **veronex-analytics에서 이미 가공된 상태**여야 한다:

| 데이터 | 가공 내용 |
|--------|----------|
| inference 이벤트 | 필요 필드만 추출, 불필요한 OTLP envelope 제거 |
| audit 이벤트 | 동일 |
| metrics | Agent에서 allowlist 필터 + 타입 분류 완료 후 전송 |

Raw OTLP payload를 그대로 Kafka에 넣고 ClickHouse에서 파싱하는 방식은 **금지** — 볼륨 증가 + 스키마 의존성.

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change agent scrape interval | `values.yaml` `scrapeIntervalMs` or `docker-compose.yml` `SCRAPE_INTERVAL_MS` | Agent env var (milliseconds) |
| Add new metric to collection | `crates/veronex-agent/src/scraper.rs` | Add prefix to `NODE_EXPORTER_ALLOWLIST` |
| Add new OTel exporter | `docker/otel/config.yaml` | Add to `exporters:` block + add to relevant `service.pipelines` |
| Change Kafka topic name | `docker/otel/config.yaml` kafka exporter `topic:` | Also update `veronex-consumer` topic subscription |
| Add new OTLP metric type | `crates/veronex-agent/src/scraper.rs` | Agent classifies in scraper; consumer handles insert |
| Add new ClickHouse chain | `docker/clickhouse/schema.sql` + `crates/veronex-consumer/src/` | MergeTree target table + consumer handler |
| Add new target MergeTree table | `docker/clickhouse/schema.sql` | Declare at top of file |
| Change HTTP SD endpoint auth | `infrastructure/inbound/http/metrics_handlers.rs` | Move route outside/inside auth middleware in `router.rs` |
| Migrate to managed Kafka | `docker/otel/config.yaml` `brokers:` + `docker-compose.yml` `REDPANDA_URL` | Address swap only -- no code changes |

## Key Files

| File | Purpose |
|------|---------|
| `docker/otel/Dockerfile` | OTel Collector image (debian-wrapped, adds wget for healthcheck) |
| `docker/otel/config.yaml` | Receiver + exporter + pipeline config (metrics, traces, logs) |
| `docker/clickhouse/schema.sql` | ClickHouse tables: MergeTree targets only (Kafka Engine 없음) |
| `docker/clickhouse/init.sh` | Init script -- substitutes `__RETENTION_*__` vars into schema.sql |
| `docker-compose.yml` | `otel-collector`, `redpanda`, `clickhouse`, `veronex`, `veronex-analytics`, `veronex-consumer` services |
| `crates/veronex-analytics/src/` | Internal analytics service (app-layer 가공 + OTel write + ClickHouse read) |
| `crates/veronex-consumer/src/` | Kafka consumer — Redpanda topic 구독 → ClickHouse HTTP INSERT |
| `crates/veronex/src/infrastructure/outbound/observability/http_observability_adapter.rs` | `HttpObservabilityAdapter` |
| `crates/veronex/src/infrastructure/outbound/observability/http_audit_adapter.rs` | `HttpAuditAdapter` |
| `crates/veronex/src/infrastructure/inbound/http/metrics_handlers.rs` | `GET /v1/metrics/targets` — two target types (server + ollama), URL normalization to `host[:port]` |
| `crates/veronex-agent/src/scraper.rs` | Metric allowlist + Prometheus text → OTLP conversion (raw values), body size limits (16MB node-exporter, 1MB Ollama) |
| `crates/veronex-agent/src/otlp.rs` | OTLP HTTP/JSON push client (3 retries, exponential backoff 2s/4s/8s) |
| `crates/veronex-agent/src/shard.rs` | Modulus sharding for multi-replica deduplication |

---

## Pipeline Overview

```
[Write Path — 모든 체인 동일 패턴]

[Chain 1: logs]
veronex --> POST /internal/ingest/inference --+
veronex --> POST /internal/ingest/audit    --> veronex-analytics (앱레이어 가공: 필요 필드 추출)
                                               +- OTel Logs SDK (OTLP gRPC) --> OTel Collector :4317
                                                                                  +- kafka/logs --> Redpanda [otel-logs]
                                                                                                    +- veronex-consumer --> otel_logs (MergeTree)
                                                                                                                            +- inference_logs
                                                                                                                            |   +- api_key_usage_hourly
                                                                                                                            +- audit_events

[Chain 2: metrics]
node-exporters (type=server) -+
                              +-> veronex-agent (allowlist 필터 + 타입분류 + OTLP push) --> OTel Collector --> kafka/metrics --> Redpanda [otel-metrics]
ollama /api/ps (type=ollama) -+                                                                                                    +- veronex-consumer --> otel_metrics_gauge (MergeTree)

[Chain 3: traces]
veronex traces --> OTel Collector --> kafka/traces --> Redpanda [otel-traces]
                                                         +- veronex-consumer --> otel_traces_raw (MergeTree)

[Read Path — ClickHouse primary, PostgreSQL fallback]
veronex --> GET /v1/usage             --> analytics_repo (ClickHouse) --> fallback: PostgreSQL inference_jobs
veronex --> GET /v1/dashboard/*       --> analytics_repo (ClickHouse) --> fallback: PostgreSQL inference_jobs
veronex --> GET /v1/audit             --> analytics_repo (ClickHouse)
```

**Key properties:**
- **단일 파이프라인 패턴**: analytics(가공) → OTel → Redpanda → `veronex-consumer` → ClickHouse (모든 체인 동일)
- **ClickHouse Kafka Engine 없음** — 외부 consumer(veronex-consumer)가 INSERT 담당
- **Redpanda Connect 없음** — 별도 런타임 의존성 제거
- **veronex-analytics = 앱레이어 가공 담당** — raw payload 금지, 필요 필드만 추출 후 OTel 전송
- **veronex crate** = Redpanda, ClickHouse 직접 의존성 없음
- **Timestamp semantics**: `timeUnixNano` = original event time (from veronex), `observedTimeUnixNano` = collector receipt time
- **Ingest validation**: Event type whitelist (`inference.completed`, `audit.action`), required field checks → 400 on invalid
- **Agent is the ONLY component that does metric processing** — allowlist filtering, type classification (gauge vs sum/counter)

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

Counter metrics (`node_cpu_seconds_total`) are sent as OTLP `sum` with `isMonotonic: true`. All other metrics are sent as OTLP `gauge`. `veronex-consumer`가 두 타입 모두 처리하여 `otel_metrics_gauge`에 INSERT.

→ Collector config, chains, MVs: `otel-pipeline-chains.md`

---

## Cross-References

- **Chains 2-3, gotchas, verification, data retention**: `infra/otel-pipeline-ops.md`
- **Observability research**: `research/infrastructure/observability.md`
- **Deploy config**: `infra/deploy.md`
