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

- [ ] `OllamaAdapter(IGpuPort)` — **서버 1개당 1 인스턴스**, url 주입:

```python
class OllamaAdapter:
    def __init__(self, server: GpuServer, client: httpx.AsyncClient):
        self.server = server
        self.client = client  # base_url = server.url

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
        resp = await self.client.get("/api/ps")
        return [(m["name"], m["size_vram"]) for m in resp.json()["models"]]

    async def unload_model(self, model_name):
        await self.client.post("/api/generate", json={
            "model": str(model_name), "keep_alive": 0,
        })

    async def health(self) -> bool:
        try:
            resp = await self.client.get("/", timeout=3.0)
            return resp.status_code == 200
        except Exception:
            return False
```

### Phase 2-1 — GPU Server Registry

- [ ] `IGpuServerRegistry` 구현 (`PostgresGpuServerRegistry`):
  - GPU 서버 목록: PostgreSQL `gpu_servers` 테이블
  - 헬스 상태 캐시: Valkey `gpu:server:{id}:status` (TTL 30s)
  - 헬스체크 백그라운드 태스크: 매 15초 전체 서버 ping

```sql
CREATE TABLE gpu_servers (
    id          VARCHAR(64) PRIMARY KEY,   -- "gpu-01"
    url         VARCHAR(255) NOT NULL,     -- "http://host:11434"
    total_vram_mb INTEGER NOT NULL DEFAULT 0,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    registered_at TIMESTAMPTZ DEFAULT now()
);
```

### Phase 2-2 — ModelAffinityRouter

- [ ] `application/use_cases/model_affinity_router.py`:

```python
class ModelAffinityRouter:
    """
    라우팅 우선순위:
    1. 해당 모델이 로드된 서버 중 active_calls 최소
    2. 모델 미로드 → free VRAM 최대 서버 선택 후 load
    3. 없으면 ResourceExhaustedError
    """
    async def route(self, model_name: ModelName) -> tuple[GpuServer, IGpuPort]:
        servers = await self.registry.list_online()

        # 모델 로드된 서버 필터
        loaded = [
            s for s in servers
            if model_name in await self._get_loaded_models(s)
        ]
        if loaded:
            # least active_calls
            target = min(loaded, key=lambda s: self._active_calls(s))
            return target, self._adapter(target)

        # 모델 미로드 → VRAM 여유 최대
        candidates = sorted(servers, key=lambda s: self._free_vram(s), reverse=True)
        if not candidates:
            raise ResourceExhaustedError("No GPU servers available")
        target = candidates[0]
        await self._adapter(target).load_model(model_name)
        return target, self._adapter(target)
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

### Phase 4 — GPU Server 등록 API

배포 환경(k8s / docker-compose / bare metal)에 무관하게 **URL만으로 연결**.

- [ ] Ollama 서버 등록 API:

```
POST   /v1/servers             → 서버 등록 {id, url, total_vram_mb}
GET    /v1/servers             → 서버 목록 (status, loaded models)
DELETE /v1/servers/{id}        → 서버 제거
POST   /v1/servers/{id}/sync   → 모델 목록 즉시 동기화
```

- [ ] 등록 정보는 **PostgreSQL `gpu_servers` 테이블에 영속 저장** → 재시작 후 자동 복구
- [ ] 등록 시 즉시 헬스체크 → 실패 시 `DEGRADED` 상태로 등록 (거부 안 함)
- [ ] `INFERQ_BOOTSTRAP_SERVERS` 환경변수: 쉼표 구분 URL 목록 → 앱 시작 시 자동 등록

```python
# startup bootstrap (main.py lifespan)
if bootstrap_urls := settings.INFERQ_BOOTSTRAP_SERVERS:
    for url in bootstrap_urls.split(","):
        url = url.strip()
        if url:
            await server_registry.register_if_not_exists(GpuServer(
                id=_url_to_id(url),  # "http://host:11434" → "host-11434"
                url=url,
            ))
```

## Done

- [ ] `IGpuPort` protocol in `application/ports/outbound/`
- [ ] `OllamaAdapter` 서버 1개당 1 인스턴스 (url 주입)
- [ ] `IGpuServerRegistry` + `ModelAffinityRouter` 구현
- [ ] GPU 서버 등록/제거/조회 API (`/v1/servers`)
- [ ] 헬스체크 백그라운드 태스크 (15초 주기)
- [ ] `keep_alive=-1` on all inference requests
- [ ] Error mapping to domain exceptions complete
