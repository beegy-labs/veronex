# Task 04: Inference Backends (Ollama + Gemini)

> MVP: OLLAMA + GEMINI. 새 백엔드 추가 = 어댑터 파일 1개 + factory case 1줄.
> 포트(IInferenceBackendPort)와 라우터(InferenceRouter)는 변경 없음.

## 확장 구조 원칙

```
IInferenceBackendPort  ← 변하지 않는 계약 (포트)
        ↑
  OllamaAdapter        ← MVP
  GeminiAdapter        ← MVP
  OpenAIAdapter        ← 추후 (파일 추가만)
  AnthropicAdapter     ← 추후 (파일 추가만)
        ↑
 BackendAdapterFactory ← BackendType → 어댑터 반환
```

---

## Steps

### Phase 1 — IInferenceBackendPort

- [ ] `application/ports/outbound/inference_backend_port.py`:

```python
class IInferenceBackendPort(Protocol):
    """
    모든 LLM 백엔드의 공통 계약.
    새 백엔드 추가 시 이 인터페이스는 변경하지 않음.
    """
    async def infer(self, job: InferenceJob) -> InferenceResult: ...
    async def stream_infer(self, job: InferenceJob) -> AsyncIterator[StreamToken]: ...
    async def list_models(self) -> list[Model]: ...
    async def health(self) -> bool: ...

    # local 전용 — cloud 백엔드는 no-op 구현
    async def load_model(self, model_name: ModelName) -> None: ...
    async def unload_model(self, model_name: ModelName) -> None: ...
    async def get_loaded_models(self) -> list[tuple[ModelName, int]]: ...
```

### Phase 2 — OllamaAdapter (OLLAMA)

- [ ] `infrastructure/outbound/backends/ollama_adapter.py`:

```python
class OllamaAdapter:
    """LlmBackend 1개당 1 인스턴스. url 주입."""
    def __init__(self, backend: LlmBackend, client: httpx.AsyncClient):
        self.backend = backend
        self.client = client  # base_url = backend.url

    async def stream_infer(self, job: InferenceJob) -> AsyncIterator[StreamToken]:
        async with self.client.stream("POST", "/api/generate", json={
            "model": str(job.model_name),
            "prompt": str(job.prompt),
            "keep_alive": -1,   # greedy: model stays loaded
            "stream": True,
        }) as resp:
            async for line in resp.aiter_lines():
                data = json.loads(line)
                yield StreamToken(data["response"], data.get("done", False))

    async def get_loaded_models(self) -> list[tuple[ModelName, int]]:
        resp = await self.client.get("/api/ps")
        return [(m["name"], m["size_vram"]) for m in resp.json()["models"]]

    async def load_model(self, model_name: ModelName) -> None:
        await self.client.post("/api/pull", json={"name": str(model_name)})

    async def unload_model(self, model_name: ModelName) -> None:
        await self.client.post("/api/generate", json={
            "model": str(model_name), "keep_alive": 0,
        })

    async def health(self) -> bool:
        try:
            return (await self.client.get("/", timeout=3.0)).status_code == 200
        except Exception:
            return False
```

### Phase 3 — GeminiAdapter (GEMINI)

- [ ] `infrastructure/outbound/backends/gemini_adapter.py`:

```python
class GeminiAdapter:
    """google-generativeai SDK 사용. api_key는 복호화 후 주입."""
    def __init__(self, backend: LlmBackend):
        import google.generativeai as genai
        genai.configure(api_key=decrypt(backend.api_key_encrypted))
        self._genai = genai

    async def stream_infer(self, job: InferenceJob) -> AsyncIterator[StreamToken]:
        model = self._genai.GenerativeModel(str(job.model_name))
        async for chunk in await model.generate_content_async(
            str(job.prompt), stream=True
        ):
            yield StreamToken(chunk.text, False)
        yield StreamToken("", True)

    async def list_models(self) -> list[Model]:
        return [
            Model(name=m.name, backend=BackendType.GEMINI,
                  vram_mb=0, status=ModelStatus.AVAILABLE)
            for m in self._genai.list_models()
            if "generateContent" in m.supported_generation_methods
        ]

    # cloud — no-op
    async def load_model(self, _): pass
    async def unload_model(self, _): pass
    async def get_loaded_models(self): return []

    async def health(self) -> bool:
        try:
            return len(await self.list_models()) > 0
        except Exception:
            return False
```

### Phase 4 — BackendAdapterFactory

- [ ] `infrastructure/outbound/backends/factory.py`:

```python
def create_adapter(backend: LlmBackend) -> IInferenceBackendPort:
    """
    BackendType → 어댑터 인스턴스.
    새 백엔드 추가: 1) 어댑터 파일 작성 2) case 1줄 추가.
    """
    match backend.backend_type:
        case BackendType.OLLAMA:
            client = httpx.AsyncClient(base_url=backend.url, timeout=120.0)
            return OllamaAdapter(backend, client)
        case BackendType.GEMINI:
            return GeminiAdapter(backend)
        case _:
            raise UnsupportedBackendError(backend.backend_type)
```

### Phase 5 — ILlmBackendRegistry + InferenceRouter

- [ ] `application/ports/outbound/llm_backend_registry.py`:

```python
class ILlmBackendRegistry(Protocol):
    async def list_online(self) -> list[LlmBackend]: ...
    async def get(self, backend_id: str) -> LlmBackend | None: ...
    async def register(self, backend: LlmBackend) -> None: ...
    async def update_status(self, backend_id: str, status: LlmBackendStatus) -> None: ...
```

- [ ] `application/use_cases/inference_router.py`:

```python
class InferenceRouter:
    """
    요청을 적절한 백엔드로 라우팅.
    - OLLAMA: model-affinity + least-connections
    - GEMINI (+ cloud): least-connections (model load 개념 없음)
    새 백엔드 추가 시 라우터 로직 변경 불필요 (cloud는 자동 처리).
    """
    async def route(self, job: InferenceJob) -> IInferenceBackendPort:
        backends = await self.registry.list_online()
        candidates = [b for b in backends if b.backend_type == job.backend_type]

        if not candidates:
            raise ResourceExhaustedError(f"No online backend for {job.backend_type}")

        # OLLAMA: model-affinity 우선
        if job.backend_type == BackendType.OLLAMA:
            return await self._route_ollama(job, candidates)

        # cloud (GEMINI, ...): least-connections
        return self._least_connections(candidates)
```

- [ ] 헬스체크 백그라운드 태스크 (15초 주기, 전체 백엔드 ping)

### Phase 6 — 백엔드 등록 API

- [ ] 배포 환경 무관하게 URL만으로 연결:

```
POST   /v1/backends              → 등록 {id, name, backend_type, url, api_key?}
GET    /v1/backends              → 목록 (status, loaded_models 포함)
DELETE /v1/backends/{id}         → 제거
POST   /v1/backends/{id}/sync    → 모델 목록 즉시 동기화
```

- [ ] PostgreSQL `llm_backends` 테이블 영속 저장 → 재시작 후 자동 복구
- [ ] `INFERQ_BOOTSTRAP_BACKENDS` 환경변수: 시작 시 자동 등록

```
# 형식: type:id:url[:api_key]
INFERQ_BOOTSTRAP_BACKENDS=ollama:gpu-01:http://host:11434,gemini:gemini-main::AIza...
```

### Phase 7 — Error Handling

- [ ] Ollama 503 → `ResourceExhaustedError`
- [ ] Gemini quota exceeded → `ResourceExhaustedError`
- [ ] Connection timeout → exponential backoff (max 3, base 1s)
- [ ] Model not found → `ModelNotFoundError`
- [ ] Unsupported backend type → `UnsupportedBackendError`

## Done

- [ ] `IInferenceBackendPort` 포트 정의 (변경 없는 계약)
- [ ] `OllamaAdapter` + `GeminiAdapter` MVP 구현
- [ ] `BackendAdapterFactory` — 새 백엔드 = case 1줄
- [ ] `ILlmBackendRegistry` + `InferenceRouter` 구현
- [ ] `/v1/backends` CRUD API (등록/제거/조회/sync)
- [ ] 헬스체크 백그라운드 태스크 (15초 주기)
- [ ] `INFERQ_BOOTSTRAP_BACKENDS` 환경변수 지원
- [ ] Error mapping 완료
