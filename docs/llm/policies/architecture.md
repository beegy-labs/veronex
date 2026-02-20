# Hexagonal Architecture Policy

> SSOT for inferq architecture | **Last Updated**: 2026-02-19

## Overview

inferq uses **Hexagonal Architecture (Ports & Adapters)** to isolate the LLM inference domain from infrastructure concerns (HTTP, Redis, GPU drivers).

## Directory Structure

```
src/
├── domain/
│   ├── entities/        # InferenceJob, InferenceResult
│   ├── value-objects/   # JobId, Prompt, StreamToken
│   └── exceptions/      # Domain-level exceptions
│
├── application/
│   ├── ports/
│   │   ├── inbound/     # Driving ports (use case interfaces)
│   │   └── outbound/    # Driven ports (infrastructure interfaces)
│   └── use-cases/       # Core business logic
│
├── infrastructure/
│   ├── inbound/
│   │   ├── http/        # FastAPI route handlers
│   │   └── sse/         # SSE streaming adapter
│   └── outbound/
│       ├── queue/       # Redis queue adapter
│       ├── gpu/         # GPU worker adapter
│       └── persistence/ # DB adapter (optional)
│
└── main.py              # Composition root
```

## Layers

### Domain Layer

- No external dependencies
- Pure Python dataclasses / Pydantic models
- Contains: entities, value objects, domain exceptions

```python
# domain/entities/inference_job.py
@dataclass
class InferenceJob:
    id: JobId
    prompt: Prompt
    status: JobStatus
    created_at: datetime
```

### Application Layer

- Depends only on domain
- Defines all ports (interfaces)
- Contains: use cases, port interfaces

```python
# application/ports/outbound/queue_port.py
class IQueuePort(ABC):
    @abstractmethod
    async def enqueue(self, job: InferenceJob) -> None: ...

    @abstractmethod
    async def dequeue(self) -> InferenceJob: ...
```

```python
# application/ports/inbound/inference_use_case.py
class IInferenceUseCase(ABC):
    @abstractmethod
    async def submit(self, prompt: str) -> JobId: ...

    @abstractmethod
    async def stream(self, job_id: JobId) -> AsyncIterator[StreamToken]: ...
```

### Infrastructure Layer

- Depends on application (implements ports)
- Contains: adapters (HTTP, SSE, Redis, GPU)
- Never contains business logic

```python
# infrastructure/outbound/queue/redis_queue_adapter.py
class RedisQueueAdapter(IQueuePort):
    async def enqueue(self, job: InferenceJob) -> None:
        await self.redis.rpush(QUEUE_KEY, job.to_json())
```

## Dependency Rule

```
infrastructure → application → domain
```

- `domain` imports nothing from other layers
- `application` imports only from `domain`
- `infrastructure` imports from `application` (to implement ports)

**Violation**: Any reverse dependency is a bug.

## Composition Root

All wiring happens in `main.py`:

```python
# main.py
queue_adapter = RedisQueueAdapter(redis_client)
gpu_adapter = GpuWorkerAdapter(model)
use_case = InferenceUseCase(queue_adapter, gpu_adapter)
http_adapter = HttpAdapter(use_case)
```

## Multi-GPU Load Balancing

inferq는 단일 GPU부터 N개 GPU 서버까지 확장 가능한 LB 역할을 합니다.

```
Client → inferq (queue + LB) → GPU Server 1 (Ollama)
                             → GPU Server 2 (Ollama)
                             → GPU Server N (Ollama)
```

### GpuServer 도메인 엔티티

```python
@dataclass
class GpuServer:
    id: str              # "gpu-01", "gpu-02", ...
    url: str             # "http://host:11434"
    total_vram_mb: int
    status: GpuServerStatus  # ONLINE / OFFLINE / DEGRADED
    last_health_check: datetime
```

### IGpuServerRegistry Port

```python
class IGpuServerRegistry(Protocol):
    async def list_online(self) -> list[GpuServer]: ...
    async def get(self, server_id: str) -> GpuServer | None: ...
    async def register(self, server: GpuServer) -> None: ...
    async def mark_offline(self, server_id: str) -> None: ...
```

### 라우팅 전략 (ModelAffinityRouter)

```
1. model이 로드된 서버 중 active_calls 최소 → 선택 (model-affinity + least-conn)
2. model 미로드, VRAM 여유 있는 서버 → 선택 후 model load
3. 모든 서버 busy/offline → 큐에서 대기
```

**단일 GPU 환경**: 서버 1개 등록으로 현재 설계 그대로 동작.
**SSOT**: `docs/llm/policies/architecture.md`

## Key Design Decisions

| Decision | Rationale |
| -------- | --------- |
| Queue + LB 통합 | inferq 자체가 LB — 외부 LB 불필요 |
| Model-affinity routing | 모델 재로드 비용 회피 (APU는 로드가 느림) |
| SSE over WebSocket | Unidirectional stream is sufficient; simpler protocol |
| Port per concern | `IQueuePort`, `IGpuPort`, `IGpuServerRegistry` separated for testability |
| Domain has no async | Pure domain logic; async lives in adapters |

## Testing Strategy

| Layer | Test Type | Mock Target |
| -------------- | -------------- | -------------------- |
| domain | Unit | Nothing (pure) |
| application | Unit | Outbound ports (mock) |
| infrastructure | Integration | Real Redis/GPU or stub |

## Port Catalog

### Inbound (Driving)

| Port | Method | Description |
| ---- | ------ | ----------- |
| `IInferenceUseCase` | `submit(prompt)` | Enqueue inference job |
| `IInferenceUseCase` | `stream(job_id)` | Stream tokens as SSE |
| `IInferenceUseCase` | `status(job_id)` | Get job status |

### Outbound (Driven)

| Port | Method | Description |
| ---- | ------ | ----------- |
| `IQueuePort` | `enqueue(job)` | Push job to queue |
| `IQueuePort` | `dequeue()` | Pop next job (blocking) |
| `IGpuPort` | `infer(job)` | Run inference on specific GPU server |
| `IGpuPort` | `stream_infer(job)` | Stream tokens from GPU server |
| `IGpuPort` | `health()` | Check server health |
| `IGpuServerRegistry` | `list_online()` | List healthy GPU servers |
| `IGpuServerRegistry` | `register(server)` | Register new GPU server |
| `IStreamPort` | `publish(token)` | Publish token to SSE channel |
| `IObservabilityPort` | `record_inference(event)` | Record metrics/logs |
| `IApiKeyRepository` | `get_by_hash(hash)` | Validate API key |
