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
| IGpuPort              | Outbound  | OllamaAdapter (per server) |
| IGpuServerRegistry    | Outbound  | PostgreSQL / Valkey     |
| IStreamPort           | Outbound  | SSE Adapter             |
| IObservabilityPort    | Outbound  | OTel / ClickHouse / stdout |
| IApiKeyRepository     | Outbound  | PostgreSQL Adapter      |

## Multi-GPU Load Balancing

inferq = queue + LB. GPU 서버가 N개로 늘어도 외부 LB 불필요.

```
Client → inferq → [ModelAffinityRouter] → GPU Server 1
                                        → GPU Server 2
                                        → GPU Server N
```

라우팅: model 로드된 서버 + least-connections 우선.
단일 GPU는 서버 1개 등록으로 동일 코드 동작.

**SSOT**: `docs/llm/policies/architecture.md`
