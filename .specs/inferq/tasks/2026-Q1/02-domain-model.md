# Task 02: Domain Model

> No external dependencies. Pure Python dataclasses + Pydantic.

## Steps

### Phase 1 — Value Objects

- [ ] `JobId` (UUID wrapper)
- [ ] `Prompt` (str, max 32k chars)
- [ ] `StreamToken` (str, is_final: bool)
- [ ] `ModelName` (str, validated)

### Phase 2 — Enums

- [ ] `JobStatus`: PENDING, QUEUED, RUNNING, COMPLETED, FAILED, CANCELLED
- [ ] `BackendType`: OLLAMA, LLAMA_CPP (future)
- [ ] `ModelStatus`: AVAILABLE, LOADING, LOADED, UNLOADING
- [ ] `ObservabilityBackend`: OTEL, CLICKHOUSE, STDOUT

### Phase 3 — Entities

- [ ] `InferenceJob`:

```python
@dataclass
class InferenceJob:
    id: JobId
    prompt: Prompt
    model_name: ModelName
    status: JobStatus
    backend: BackendType
    created_at: datetime
    started_at: datetime | None = None
    completed_at: datetime | None = None
    error: str | None = None
```

- [ ] `Model`:

```python
@dataclass
class Model:
    name: ModelName
    backend: BackendType
    vram_mb: int           # estimated VRAM requirement
    status: ModelStatus
    last_used_at: datetime | None = None
    active_calls: int = 0  # LRU eviction: never evict if > 0
```

- [ ] `InferenceResult`:

```python
@dataclass
class InferenceResult:
    job_id: JobId
    prompt_tokens: int
    completion_tokens: int
    latency_ms: int
    tokens: list[str]
```

### Phase 4 — Domain Exceptions

- [ ] `ModelNotFoundError`
- [ ] `ModelLoadTimeoutError`
- [ ] `InferenceTimeoutError`
- [ ] `ResourceExhaustedError`
- [ ] `JobNotFoundError`

## Verify

```bash
python -c "from src.domain.entities.inference_job import InferenceJob; print('OK')"
```

## Done

- [ ] All entities are pure dataclasses (no framework imports)
- [ ] All enums defined
- [ ] All domain exceptions defined
