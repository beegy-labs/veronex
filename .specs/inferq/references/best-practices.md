# Best Practices Reference

> Research-based implementation decisions | **Last Updated**: 2026-02-19

## Queue & Worker

### Library: ARQ (asyncio-native)

| Option | Decision | Reason |
| ------ | -------- | ------ |
| ARQ | ✅ Selected | asyncio-native, 7x faster than RQ, natural fit for FastAPI |
| Celery | ❌ Skip | sync-first, requires extra config for async |
| Taskiq | Alternative | Celery patterns + async, better under heavy load |

```python
# ARQ worker pattern
async def inference_worker(ctx: dict, job_id: str) -> None:
    ...

class WorkerSettings:
    functions = [inference_worker]
    redis_settings = RedisSettings(host="valkey", port=6379)
    max_jobs = 1  # single GPU: serial processing
```

**Key ARQ settings for single GPU:**
- `max_jobs = 1` — only one inference at a time
- `job_timeout = 300` — 5min max per inference
- Idempotency: use job_id as deduplication key

---

## Ollama Integration

### Critical Environment Variables

| Variable | Recommended Value | Effect |
| -------- | ---------------- | ------ |
| `OLLAMA_KEEP_ALIVE` | `-1` | Keep models loaded permanently (greedy allocation) |
| `OLLAMA_NUM_PARALLEL` | `1` | Single GPU, no parallel per model |
| `OLLAMA_MAX_QUEUE` | `512` | Default, inferq manages queue above this |
| `OLLAMA_FLASH_ATTENTION` | `1` | Reduces VRAM usage significantly |

### Model Loading Strategy (APU: 96GB unified memory)

- **Greedy Allocation**: load model if memory available, keep loaded
- **LRU Eviction**: evict least recently used when memory full
- **Active-call protection**: never evict model with in-flight request (LocalAI pattern)
- **Retry on busy**: max_retries=30, interval=1s before failing eviction

```python
# LRU eviction with active-call protection
async def evict_for_model(required_vram_mb: int) -> bool:
    candidates = [m for m in loaded_models if m.active_calls == 0]
    candidates.sort(key=lambda m: m.last_used_at)  # LRU
    for candidate in candidates:
        if free_vram + candidate.vram_mb >= required_vram_mb:
            await unload_model(candidate)
            return True
    return False  # all models busy, wait and retry
```

### Ollama keep_alive per request

```python
# Set keep_alive=-1 per request to override server default
payload = {
    "model": model_name,
    "prompt": prompt,
    "keep_alive": -1,  # keep in memory after this request
    "stream": True,
}
```

---

## SSE Streaming

### Library: sse-starlette

```python
from sse_starlette.sse import EventSourceResponse

@router.get("/stream/{job_id}")
async def stream(job_id: str, request: Request):
    async def generator():
        async for token in use_case.stream(job_id):
            if await request.is_disconnected():
                break  # save GPU cycles on client disconnect
            yield {"data": token}
        yield {"data": "[DONE]"}

    return EventSourceResponse(generator())
```

**Critical production configs:**

| Issue | Fix |
| ----- | --- |
| Nginx buffers SSE | `proxy_buffering off` + `X-Accel-Buffering: no` header |
| Cilium Gateway (k8s) | Same — disable buffering at ingress level |
| Multiple SSE clients | HTTP/2 (multiplexing) |
| Proxy drops idle | Send heartbeat ping every 15s |

---

## Hexagonal Architecture (FastAPI)

### Ports: Protocol over ABC

```python
# Use typing.Protocol (structural subtyping) — no inheritance required
from typing import Protocol, runtime_checkable

@runtime_checkable
class IGpuPort(Protocol):
    async def infer(self, job: InferenceJob) -> InferenceResult: ...
    async def stream_infer(self, job: InferenceJob) -> AsyncIterator[StreamToken]: ...
```

### Composition Root via FastAPI Lifespan

```python
@asynccontextmanager
async def lifespan(app: FastAPI):
    # Wire all adapters once at startup
    queue = RedisQueueAdapter(valkey_client)
    gpu = OllamaAdapter(ollama_url)
    obs = OtelObservabilityAdapter(otel_endpoint)  # or ClickHouseObservabilityAdapter
    use_case = InferenceUseCase(queue, gpu, obs)
    app.state.use_case = use_case
    yield
    await cleanup()
```

---

## PostgreSQL (Job State)

### SQLAlchemy 2.0 Async

```python
engine = create_async_engine(
    DATABASE_URL,
    pool_size=10,
    max_overflow=20,
    pool_timeout=30,
    pool_recycle=1800,
    pool_pre_ping=True,
)
async_session = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)
```

### Job Status Enum Caution

- Use `alembic-postgresql-enum` package for enum migrations
- `ALTER TYPE ADD VALUE` must run outside transaction block (autocommit)
- Never delete enum values — add only

---

## ClickHouse (Analytics)

### Table Engine: MergeTree

```sql
CREATE TABLE inference_logs (
    event_time     DateTime64(3),
    request_id     UUID,
    model_name     LowCardinality(String),
    prompt_tokens  UInt32,
    completion_tokens UInt32,
    latency_ms     UInt32,
    backend        LowCardinality(String),  -- ollama, llama_cpp
    status         LowCardinality(String),  -- success, error, timeout
    error_msg      String DEFAULT ''
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(event_time)
ORDER BY (event_time, model_name, request_id);
```

**Key rules:**
- `MergeTree` for immutable append-only events (NOT ReplacingMergeTree)
- Partition by day; ORDER BY starts with timestamp
- `LowCardinality(String)` for low-cardinality fields (model, status, backend)
- Avoid `SELECT ... FINAL` — forces single-threaded merge, very slow

### Python Client: clickhouse-connect (async)

```python
import clickhouse_connect

client = await clickhouse_connect.get_async_client(
    host=CLICKHOUSE_HOST,
    port=8123,
    username=CLICKHOUSE_USER,
    password=CLICKHOUSE_PASSWORD,
)
await client.insert("inference_logs", rows, column_names=[...])
```

---

## OpenTelemetry

### Metrics to Expose (/metrics)

| Metric | Type | Labels |
| ------ | ---- | ------ |
| `inferq_requests_total` | Counter | model, status |
| `inferq_queue_depth` | Gauge | - |
| `inferq_inference_duration_ms` | Histogram | model, backend |
| `inferq_tokens_total` | Counter | model, type (prompt/completion) |
| `inferq_model_load_duration_ms` | Histogram | model |
| `inferq_gpu_memory_used_mb` | Gauge | - |

### FastAPI Auto-instrumentation

```python
from opentelemetry.instrumentation.fastapi import FastAPIInstrumentor
FastAPIInstrumentor.instrument_app(app)
```

---

## Observability Backend Selection

| Config | Adapter Used |
| ------ | ------------ |
| `OBSERVABILITY_BACKEND=otel` | OtelObservabilityAdapter → OTel Collector |
| `OBSERVABILITY_BACKEND=clickhouse` | ClickHouseObservabilityAdapter → direct write |
| `OBSERVABILITY_BACKEND=stdout` | StdoutObservabilityAdapter (dev/fallback) |
