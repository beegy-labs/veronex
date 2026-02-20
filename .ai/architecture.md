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
| IInferenceBackendPort | Outbound  | OllamaAdapter / GeminiAdapter / OpenAIAdapter / AnthropicAdapter |
| ILlmBackendRegistry   | Outbound  | PostgreSQL / Valkey     |
| IStreamPort           | Outbound  | SSE Adapter             |
| IObservabilityPort    | Outbound  | OTel / ClickHouse / stdout |
| IApiKeyRepository     | Outbound  | PostgreSQL Adapter      |

## Multi-Backend Load Balancing

inferq = queue + LB + multi-backend gateway.

```
Client → inferq → [ModelAffinityRouter] → Ollama (local GPU)  ← OLLAMA
                                        → Gemini API           ← GEMINI (1차)
                                        → OpenAI API           ← OPENAI
                                        → Anthropic API        ← ANTHROPIC
                                        → Any OpenAI-compat    ← OPENAI_COMPATIBLE
```

- 모든 백엔드 = `IInferenceBackendPort` 동일 포트
- 로컬(Ollama): model-affinity + least-connections 라우팅
- 클라우드 API: least-connections (model load 개념 없음)
- 백엔드 등록: API (`POST /v1/backends`) — 코드/재배포 불필요
- 배포 환경 무관: URL + api_key(선택)만으로 연결

**SSOT**: `docs/llm/policies/architecture.md`
