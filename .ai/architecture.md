# Architecture

> Hexagonal Architecture overview | **Last Updated**: 2026-02-25

## Structure

```
crates/inferq/src/
├── domain/          # Entities, value objects, enums (no deps)
├── application/     # Use cases + ports (traits)
│   ├── ports/
│   │   ├── inbound/   # InferenceUseCase
│   │   └── outbound/  # Repositories, registries, adapters
│   └── use_cases/
├── infrastructure/  # Adapters (implements ports)
│   ├── inbound/http/  # Axum handlers, middleware, router
│   └── outbound/      # Postgres, Valkey, Ollama, Gemini, OTel
└── main.rs          # Composition root (wires everything)
```

## Dependency Rule

```
infrastructure → application → domain
(Never reverse. Domain knows nothing outside itself.)
```

## Key Ports

| Port                   | Direction | Implemented By                       |
| ---------------------- | --------- | ------------------------------------ |
| `InferenceUseCase`     | Inbound   | HTTP/SSE handlers                    |
| `InferenceBackendPort` | Outbound  | OllamaAdapter / GeminiAdapter        |
| `LlmBackendRegistry`   | Outbound  | PostgresBackendRegistry              |
| `GpuServerRegistry`    | Outbound  | PostgresGpuServerRegistry            |
| `JobRepository`        | Outbound  | PostgresJobRepository                |
| `ApiKeyRepository`     | Outbound  | PostgresApiKeyRepository             |
| `ObservabilityPort`    | Outbound  | ClickHouseObservabilityAdapter       |
| `ModelManagerPort`     | Outbound  | OllamaModelManager (LRU eviction)    |

## Multi-Backend Routing

```
Client → POST /v1/inference
       → DynamicBackendRouter
         → VRAM check → claim best GPU → tokio::spawn
         → OllamaAdapter | GeminiAdapter
       → SSE stream → Client
```

**SSOT**: `docs/llm/policies/architecture.md`
