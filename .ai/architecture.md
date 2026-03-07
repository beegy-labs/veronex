# Architecture
> CDD Tier 1 — Hexagonal Architecture pointer (≤50 lines) | **Last Updated**: 2026-03-07

## Structure

```
crates/veronex/src/
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
| `ObservabilityPort` | Outbound | HttpObservabilityAdapter (fail-open) |

## Background Loops

| Loop | Interval | Purpose |
|------|----------|---------|
| `sync_loop` | base tick 30s (per-provider sync_interval ~300s) | Unified: health + model sync + VRAM probe + LLM analysis |
| `health_checker` | 30 s | Provider health + agent metrics + thermal auto-detect |
| `queue_dispatcher` | Lua priority pop | 3-queue dispatch + model filter + stickiness + gate chain |
| `session_grouping` | 24 h | Batch conversation_id assignment |

**SSOT**: `docs/llm/policies/architecture.md`
