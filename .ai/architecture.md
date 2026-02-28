# Architecture

> Hexagonal Architecture overview | **Last Updated**: 2026-02-28

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

| Port                           | Direction | Implemented By                      |
| ------------------------------ | --------- | ------------------------------------ |
| `InferenceUseCase`             | Inbound   | HTTP handlers (inference + OpenAI)   |
| `InferenceBackendPort`         | Outbound  | OllamaAdapter / GeminiAdapter        |
| `LlmBackendRegistry`           | Outbound  | PostgresBackendRegistry              |
| `GpuServerRegistry`            | Outbound  | PostgresGpuServerRegistry            |
| `JobRepository`                | Outbound  | PostgresJobRepository                |
| `ApiKeyRepository`             | Outbound  | PostgresApiKeyRepository             |
| `ObservabilityPort`            | Outbound  | ClickHouseObservabilityAdapter       |
| `ModelManagerPort`             | Outbound  | OllamaModelManager (LRU eviction)    |
| `GeminiPolicyRepository`       | Outbound  | PostgresGeminiPolicyRepository       |
| `GeminiSyncConfigRepository`   | Outbound  | PostgresGeminiSyncConfigRepository   |
| `GeminiModelRepository`        | Outbound  | PostgresGeminiModelRepository        |
| `BackendModelSelectionRepository` | Outbound | PostgresBackendModelSelectionRepository |

## Inference Flow

```
Client → POST /v1/chat/completions  (OpenAI-compatible)
       → InferenceUseCaseImpl::submit(source: Api|Test)
       → Valkey RPUSH veronex:queue:jobs        (source=api)
                   or veronex:queue:jobs:test   (source=test)
       → SSE stream → Client

queue_dispatcher_loop (BLPOP [API queue, test queue]):
  API queue is always polled first (BLPOP key-order guarantee)
  → DynamicBackendRouter::dispatch()
  → OllamaAdapter | GeminiAdapter
  → RedpandaObservabilityAdapter (record_inference → Redpanda → ClickHouse)

Test reconnect:
  GET /v1/jobs/{id}/stream → OpenAI SSE replay (completed or live)
  Frontend: localStorage veronex:test:tab:{tabKey} → auto-reconnect on mount
```

## AppState (main.rs — Composition Root)

All state injected via `Arc<dyn Trait>` into Axum `State<AppState>`:
- `use_case`, `job_repo`, `api_key_repo`, `backend_registry`, `gpu_server_registry`
- `ollama_model_repo`, `ollama_sync_job_repo`
- `gemini_policy_repo`, `gemini_sync_config_repo`, `gemini_model_repo`
- `model_selection_repo`, `pg_pool` (direct ClickHouse queries)
- `valkey_pool` (rate limiting + queue)

**SSOT**: `docs/llm/policies/architecture.md`
