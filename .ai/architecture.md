# Architecture

> CDD Tier 1 — Hexagonal Architecture pointer (≤50 lines) | **Last Updated**: 2026-03-03

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

## Key Ports

| Port | Direction | Adapter |
| ---- | --------- | ------- |
| `InferenceUseCase` | Inbound | HTTP handlers |
| `InferenceBackendPort` | Outbound | OllamaAdapter, GeminiAdapter |
| `LlmProviderRegistry` | Outbound | CachingProviderRegistry (5s TTL) |
| `JobRepository` | Outbound | PostgresJobRepository |
| `ApiKeyRepository` | Outbound | PostgresApiKeyRepository |
| `AccountRepository` | Outbound | PostgresAccountRepository |
| `AuditPort` | Outbound | HttpAuditAdapter (fail-open) |
| `ObservabilityPort` | Outbound | HttpObservabilityAdapter (fail-open) |
| `QueuePort` | Outbound | Valkey (BLPOP/RPUSH) |
| `MessageStore` | Outbound | S3MessageStore (MinIO/AWS) |

## Background Loops

| Loop | Interval | Purpose |
|------|----------|---------|
| `health_checker` | 30 s | Backend online/offline + thermal |
| `capacity_analysis` | 30 s tick | KV cache calc + slot recommendation |
| `queue_dispatcher` | BLPOP 5s | VRAM-sorted job dispatch |
| `session_grouping` | 24 h | Batch conversation_id assignment |

**SSOT**: `docs/llm/policies/architecture.md`
