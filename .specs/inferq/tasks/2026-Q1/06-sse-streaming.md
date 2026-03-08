# Task 06: SSE Streaming Endpoint

> Ref: best-practices.md → SSE Streaming section
> Library: sse-starlette (W3C compliant, production-ready)

## Steps

### Phase 1 — Inbound Port

- [ ] Define `IInferenceUseCase(Protocol)`:

```python
class IInferenceUseCase(Protocol):
    async def submit(self, prompt: str, model_name: str) -> JobId: ...
    async def stream(self, job_id: JobId) -> AsyncIterator[StreamToken]: ...
    async def get_status(self, job_id: JobId) -> JobStatus: ...
    async def cancel(self, job_id: JobId) -> None: ...
```

### Phase 2 — HTTP Adapter

- [ ] POST `/v1/inference` → enqueue job, return `{job_id}`
- [ ] GET `/v1/inference/{job_id}/stream` → SSE stream
- [ ] GET `/v1/inference/{job_id}/status` → job status
- [ ] DELETE `/v1/inference/{job_id}` → cancel

```python
@router.get("/v1/inference/{job_id}/stream")
async def stream_inference(job_id: str, request: Request):
    async def generator():
        # Heartbeat every 15s to keep proxy connection alive
        heartbeat_task = asyncio.create_task(_heartbeat(request))
        try:
            async for token in use_case.stream(JobId(job_id)):
                if await request.is_disconnected():
                    break  # client left — stop GPU work
                yield {"event": "token", "data": token.value}
            yield {"event": "done", "data": ""}
        finally:
            heartbeat_task.cancel()

    return EventSourceResponse(
        generator(),
        headers={"X-Accel-Buffering": "no"},  # disable nginx buffering
    )
```

### Phase 3 — Queue Position Endpoint

- [ ] GET `/v1/inference/{job_id}/position` → queue position (1-based)

### Phase 4 — Model Management Endpoints

- [ ] GET `/v1/models` → list all models (with status, vram)
- [ ] POST `/v1/models/sync` → trigger sync from Ollama

## Verify

```bash
curl -X POST http://localhost:8000/v1/inference \
  -d '{"prompt": "Hello", "model": "llama3.2"}'
# returns {"job_id": "..."}

curl -N http://localhost:8000/v1/inference/{job_id}/stream
# streams tokens
```

## Done

- [ ] `X-Accel-Buffering: no` header on all SSE responses
- [ ] Disconnect detection stops GPU processing
- [ ] Heartbeat prevents proxy timeout
- [ ] Queue position endpoint works
