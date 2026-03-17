# Architecture
> CDD Layer 1 — Hexagonal Architecture pointer (≤50 lines) | **Last Updated**: 2026-03-15

## Structure

```
crates/veronex/src/
├── domain/          # Entities, value objects, enums (no deps)
├── application/     # Use cases + ports (traits)
│   ├── ports/
│   │   ├── inbound/   # InferenceUseCase
│   │   └── outbound/  # Repositories, registries, adapters
│   └── use_cases/inference/  # mod, use_case, dispatcher, runner, helpers
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

## Key Ports (subset — full list in SSOT)

| Port | Direction | Adapter |
| ---- | --------- | ------- |
| `InferenceUseCase` | Inbound | HTTP handlers |
| `InferenceProviderPort` | Outbound | OllamaAdapter, GeminiAdapter |
| `ProviderDispatchPort` | Outbound | ConcreteProviderDispatch |
| `LlmProviderRegistry` | Outbound | CachingProviderRegistry (5s TTL) |
| `JobRepository` | Outbound | PostgresJobRepository |
| `ApiKeyRepository` | Outbound | PostgresApiKeyRepository |
| `AuditPort` | Outbound | HttpAuditAdapter (fail-open) |
| `ImageStore` | Outbound | S3ImageStore (WebP, separate bucket) |
| `ObservabilityPort` | Outbound | HttpObservabilityAdapter (fail-open) |

## Background Loops (12)

sync_loop(30s), health_checker(30s), queue_dispatcher(500ms), placement_planner(5s),
job_sweeper(5m), promote_overdue(30s), demand_resync(60s), queue_wait_cancel(30s),
reaper(60s), job_event_subscriber, cancel_subscriber, session_grouping(24h).

**SSOT**: `docs/llm/policies/architecture.md`
