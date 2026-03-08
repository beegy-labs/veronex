# Task 03: ARQ Queue Worker

> Ref: best-practices.md → Queue & Worker section

## Steps

### Phase 1 — IQueuePort (outbound port)

- [ ] Define `IQueuePort` protocol:

```python
class IQueuePort(Protocol):
    async def enqueue(self, job: InferenceJob) -> None: ...
    async def get_job(self, job_id: JobId) -> InferenceJob | None: ...
    async def update_status(self, job_id: JobId, status: JobStatus) -> None: ...
    async def get_queue_depth(self) -> int: ...
```

### Phase 2 — ARQ Adapter

- [ ] Implement `ArqQueueAdapter(IQueuePort)`:
  - `enqueue`: `await queue.enqueue_job("inference_worker", job_id=str(job.id))`
  - store job metadata in Valkey (TTL 24h)
  - `get_queue_depth`: use ARQ job count API

### Phase 3 — Worker

- [ ] `src/infrastructure/outbound/queue/worker.py`:

```python
async def inference_worker(ctx: dict, job_id: str) -> None:
    use_case: IInferenceUseCase = ctx["use_case"]
    await use_case.process(JobId(job_id))

class WorkerSettings:
    functions = [inference_worker]
    max_jobs = 1          # single GPU: serial only
    job_timeout = 300     # 5min max per job
    keep_result = 3600    # keep result 1hr in Valkey
    retry_jobs = True
    max_tries = 3
```

### Phase 4 — SSE Result Channel

- [ ] Store tokens in Valkey list: `tokens:{job_id}`
- [ ] Publish completion signal: `complete:{job_id}`
- [ ] SSE endpoint reads from Valkey stream (BLPOP or pub/sub)

## Verify

```bash
arq src.infrastructure.outbound.queue.worker.WorkerSettings
```

## Done

- [ ] `IQueuePort` protocol defined in `application/ports/outbound/`
- [ ] `ArqQueueAdapter` implements the port
- [ ] Worker processes one job at a time (`max_jobs=1`)
- [ ] Token streaming via Valkey channel works
