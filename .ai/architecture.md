# Architecture

> Hexagonal Architecture overview | **Last Updated**: 2026-02-19

## Structure

```
src/
├── domain/          # Core entities & value objects (no deps)
├── application/     # Use cases + ports (interfaces)
│   ├── ports/
│   │   ├── inbound/   # Driving ports (IInferenceUseCase, etc.)
│   │   └── outbound/  # Driven ports (IQueuePort, IGpuPort, etc.)
│   └── use-cases/
├── infrastructure/  # Adapters (implements ports)
│   ├── inbound/     # HTTP, SSE, WebSocket adapters
│   └── outbound/    # Redis, GPU worker, DB adapters
└── main.py          # Composition root (wires everything)
```

## Dependency Rule

```
infrastructure → application → domain
(Never reverse. Domain knows nothing outside itself.)
```

## Key Ports

| Port                  | Direction | Implemented By          |
| --------------------- | --------- | ----------------------- |
| IInferenceUseCase     | Inbound   | HTTP/SSE Adapter        |
| IQueuePort            | Outbound  | Valkey Adapter          |
| IInferenceBackendPort | Outbound  | OllamaAdapter (MVP) / GeminiAdapter (MVP) / *(추후 추가)* |
| ILlmBackendRegistry   | Outbound  | PostgreSQL / Valkey     |
| IStreamPort           | Outbound  | SSE Adapter             |
| IObservabilityPort    | Outbound  | OTel / ClickHouse / stdout |
| IApiKeyRepository     | Outbound  | PostgreSQL Adapter      |

## Multi-Backend Load Balancing

inferq = queue + LB + multi-backend gateway.

```
Client → inferq → [InferenceRouter] → OllamaAdapter  (OLLAMA, MVP)
                                    → GeminiAdapter   (GEMINI, MVP)
                                    → *(새 어댑터 파일 1개 + factory case 1줄로 확장)*
```

- 모든 백엔드 = `IInferenceBackendPort` 동일 포트 (포트는 변경 없음)
- 로컬(Ollama): model-affinity + least-connections 라우팅
- 클라우드(Gemini, ...): least-connections
- 백엔드 등록: `POST /v1/backends` API — 코드/재배포 불필요
- 배포 환경 무관: URL + api_key(선택)만으로 연결

**SSOT**: `docs/llm/policies/architecture.md`
