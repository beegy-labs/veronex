# Task 04: Ollama Adapter (IGpuPort)

> Ref: best-practices.md → Ollama Integration section

## Steps

### Phase 1 — IGpuPort Protocol

- [ ] Define in `application/ports/outbound/gpu_port.py`:

```python
class IGpuPort(Protocol):
    async def infer(self, job: InferenceJob) -> InferenceResult: ...
    async def stream_infer(self, job: InferenceJob) -> AsyncIterator[StreamToken]: ...
    async def list_models(self) -> list[Model]: ...
    async def load_model(self, model_name: ModelName) -> None: ...
    async def unload_model(self, model_name: ModelName) -> None: ...
    async def get_loaded_models(self) -> list[tuple[ModelName, int]]: ...  # (name, vram_mb)
    async def health(self) -> bool: ...
```

### Phase 2 — OllamaAdapter

- [ ] Implement `OllamaAdapter(IGpuPort)` using `httpx.AsyncClient`:

```python
class OllamaAdapter:
    BASE_URL = "http://ollama:11434"

    async def stream_infer(self, job) -> AsyncIterator[StreamToken]:
        async with self.client.stream("POST", "/api/generate", json={
            "model": str(job.model_name),
            "prompt": str(job.prompt),
            "keep_alive": -1,        # greedy: keep model loaded
            "stream": True,
        }) as response:
            async for line in response.aiter_lines():
                data = json.loads(line)
                yield StreamToken(data["response"], data.get("done", False))

    async def get_loaded_models(self):
        resp = await self.client.get("/api/ps")  # Ollama loaded models API
        return [(m["name"], m["size_vram"]) for m in resp.json()["models"]]

    async def unload_model(self, model_name):
        # keep_alive=0 triggers immediate unload
        await self.client.post("/api/generate", json={
            "model": str(model_name),
            "keep_alive": 0,
        })
```

### Phase 3 — Error Handling

- [ ] 503 from Ollama → `ResourceExhaustedError` (server overloaded)
- [ ] Connection timeout → retry with exponential backoff (max 3)
- [ ] Model not found (404) → `ModelNotFoundError`

## Verify

```bash
# Ollama must be running
curl http://localhost:11434/api/tags
python -c "from src.infrastructure.outbound.gpu.ollama_adapter import OllamaAdapter"
```

## Done

- [ ] `IGpuPort` protocol in `application/ports/outbound/`
- [ ] `OllamaAdapter` implements all port methods
- [ ] `keep_alive=-1` on all inference requests
- [ ] `keep_alive=0` for forced unload
- [ ] Error mapping to domain exceptions complete
