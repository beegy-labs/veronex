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

| Port           | Direction | Implemented By       |
| -------------- | --------- | -------------------- |
| IInferenceUseCase | Inbound | HTTP/SSE Adapter    |
| IQueuePort     | Outbound  | Redis Adapter        |
| IGpuPort       | Outbound  | GPU Worker Adapter   |
| IStreamPort    | Outbound  | SSE Adapter          |

**SSOT**: `docs/llm/policies/architecture.md`
