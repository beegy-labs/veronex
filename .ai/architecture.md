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
| `ObservabilityPort` | Outbound | HttpObservabilityAdapter (fail-open) |

## Background Loops

| Loop | Interval | Purpose |
|------|----------|---------|
| `sync_loop` | base tick 30s (per-provider sync_interval ~300s) | Unified: health + model sync + VRAM probe + LLM analysis |
| `health_checker` | 30s | Provider health + hw_metrics fetch + thermal auto-detect |
| `queue_dispatcher` | 500ms (empty sleep) | ZSET peek + Rust scoring — single ZSET + 4-stage filter + gate chain |
| `placement_planner` | 5s | Scale-Out / standby / preload / evict (Valkey only) |
| `job_sweeper` | 5 min | Remove orphaned in-memory DashMap entries for cancelled jobs |
| `promote_overdue` | 30s | Elevate ZSET-stale jobs to EMERGENCY_BONUS score (anti-starvation) |
| `demand_resync` | 60s | ZSET ground-truth recount — corrects demand_counter drift |
| `reaper` | 60s | Heartbeat check + processing-list reap (crash recovery) (Valkey only) |
| `job_event_subscriber` | event-driven | Cross-instance job status relay via Valkey pub/sub (Valkey only) |
| `cancel_subscriber` | event-driven | Pattern-subscribe `cancel:*` → fire local cancel_notify (Valkey only) |
| `session_grouping` | 24h | Batch conversation_id assignment |

**SSOT**: `docs/llm/policies/architecture.md`
