# Task 08: Observability Adapters

> Ref: best-practices.md → Observability Backend Selection
> Config-driven: OBSERVABILITY_BACKEND=otel|clickhouse|stdout

## Steps

### Phase 1 — IObservabilityPort

- [ ] Define in `application/ports/outbound/observability_port.py`:

```python
class IObservabilityPort(Protocol):
    async def record_inference(self, event: InferenceEvent) -> None: ...
    async def record_token_usage(self, usage: TokenUsage) -> None: ...
    async def record_model_load(self, model: ModelName, duration_ms: int) -> None: ...
    async def record_gpu_metrics(self, metrics: GpuMetrics) -> None: ...
```

### Phase 2 — StdoutAdapter (default / dev)

- [ ] JSON log to stdout
- [ ] Zero dependencies, always works

### Phase 3 — OtelAdapter

- [ ] Metrics via MeterProvider:

```python
meter = get_meter("inferq")
requests_counter = meter.create_counter("inferq_requests_total")
tokens_counter   = meter.create_counter("inferq_tokens_total")
latency_hist     = meter.create_histogram("inferq_inference_duration_ms")
queue_depth      = meter.create_gauge("inferq_queue_depth")
gpu_memory_gauge = meter.create_gauge("inferq_gpu_memory_used_mb")
```

- [ ] FastAPI auto-instrumentation: `FastAPIInstrumentor.instrument_app(app)`
- [ ] Export to: `OTEL_EXPORTER_OTLP_ENDPOINT` env var

### Phase 4 — ClickHouseAdapter (k8s without OTel)

- [ ] Schema:

```sql
CREATE TABLE inference_logs (
    event_time        DateTime64(3),
    request_id        UUID,
    model_name        LowCardinality(String),
    prompt_tokens     UInt32,
    completion_tokens UInt32,
    latency_ms        UInt32,
    backend           LowCardinality(String),
    status            LowCardinality(String),
    error_msg         String DEFAULT ''
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(event_time)
ORDER BY (event_time, model_name, request_id);
```

- [ ] Async insert via `clickhouse-connect`:

```python
await ch_client.insert("inference_logs", rows, column_names=[...])
```

### Phase 5 — /metrics endpoint (Prometheus)

- [ ] Expose `prometheus_client` metrics at `/metrics`
- [ ] Prometheus scrape config added to docker-compose

### Phase 6 — Adapter Selection

- [ ] `settings.OBSERVABILITY_BACKEND` → factory selects adapter
- [ ] Fallback chain: otel → clickhouse → stdout

## Verify

```bash
OBSERVABILITY_BACKEND=stdout python -m src.main
# check stdout JSON logs

OBSERVABILITY_BACKEND=otel OTEL_EXPORTER_OTLP_ENDPOINT=http://otel:4317 python -m src.main
# check OTel collector

curl http://localhost:8000/metrics
# check Prometheus metrics
```

## Done

- [ ] All 3 adapters implement `IObservabilityPort`
- [ ] `/metrics` endpoint returns Prometheus format
- [ ] `OBSERVABILITY_BACKEND` env var switches adapter at startup
- [ ] ClickHouse MergeTree schema created and tested
