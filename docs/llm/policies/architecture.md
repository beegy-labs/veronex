# Hexagonal Architecture Policy

> SSOT | **Last Updated**: 2026-04-28 | Classification: Constitutional
> Code patterns and templates → `policies/patterns.md`

## Vision

Veronex is an **autonomous intelligence scheduler/gateway** for N llama-server nodes:

- **Cluster-wide optimization**: maximize total throughput across all servers, not individual server performance
- **Dynamic model allocation**: compute optimal "model combination + concurrent request count" per server in real-time
- **Multi-model co-residence**: when VRAM allows, load multiple models simultaneously for parallel processing; when insufficient, FIFO + model locality to minimize switching cost
- **3-phase adaptive learning**: Cold Start (`num_parallel` top-down, multi-model `committed_parallel` guard) → AIMD (TPS+p95 per model, capped at `num_parallel`) → LLM Batch (all-model combination tuning)
- **Thermal protection**: auto decelerate → block → cooldown → gradual recovery (per-provider thresholds, auto-detected from GPU vendor)
- **Self-healing**: circuit breaker per provider, crash recovery via Valkey, queue reaper for orphaned jobs

## Overview

Veronex uses **Hexagonal Architecture (Ports & Adapters)** to isolate the LLM inference domain from infrastructure concerns (HTTP, Valkey, Postgres, OTel).

## Directory Structure

```
crates/veronex/src/
├── domain/
│   ├── entities/        # InferenceJob, LlmProvider, GpuServer, ApiKey, …
│   ├── enums.rs         # JobStatus, ProviderType, LlmProviderStatus, …
│   ├── services/        # Pure domain logic (message_hashing, api_key_generator)
│   ├── constants.rs     # SSOT for domain-layer constants (TPM, job lifecycle, queue timing)
│   ├── errors.rs        # DomainError (Validation, NotFound, Internal, …)
│   └── value_objects.rs # JobId, Prompt, ModelName
│
├── application/
│   ├── ports/
│   │   ├── inbound/     # InferenceUseCase (driving port)
│   │   └── outbound/    # all outbound port traits
│   └── use_cases/
│       └── inference/   # mod.rs (JobEntry), use_case.rs, dispatcher.rs, runner.rs, helpers.rs
│
├── infrastructure/
│   ├── inbound/http/    # Axum handlers, middleware, router, AppState, error.rs
│   └── outbound/
│       ├── persistence/ # Postgres adapters (one per port)
│       ├── llama-server/      # LlamaServerAdapter
│       ├── gemini/      # GeminiAdapter
│       ├── provider_router.rs  # DynamicProviderRouter (VRAM-aware)
│       ├── health_checker.rs   # 30s background health checker (+ thermal throttle update)
│       ├── model_manager/      # LlamaServerModelManager (disabled — VramPool manages lifecycle)
│       ├── observability/      # HttpObservabilityAdapter + HttpAuditAdapter (fail-open → veronex-analytics)
│       ├── analytics/          # HttpAnalyticsClient (GET from veronex-analytics)
│       ├── pubsub/             # Cross-instance relay (Valkey Streams + Pub/Sub) + reaper (crash recovery)
│       ├── s3/                 # S3ImageStore, S3MessageStore, WebP conversion
│       ├── session_grouping.rs # Conversation session grouping (background loop)
│       ├── queue_maintenance.rs # Queue reaper, orphan cleanup
│       ├── valkey_keys.rs      # pk-aware shims (init_prefix/pk + delegates) — canonical keys live in domain/constants.rs
│       └── capacity/           # VramPool, DistributedVramPool, ThermalThrottleMap, CapacityAnalyzer
│
└── main.rs              # Composition root — wires all adapters
```

## Dependency Rule

```
infrastructure → application → domain
```

- `domain` imports nothing from other layers
- `application` imports only from `domain`
- `infrastructure` imports from `application` (implements port traits)

Violation = compile error (Rust enforces this naturally).

## Layers

| Layer | Rules |
|-------|-------|
| Domain | No dependencies, no async, no I/O. Pure structs/enums |
| Application | Depends only on `domain`. Defines port traits (`#[async_trait]`) + use case impl |
| Infrastructure | Implements ports (adapters). No business logic |

## Composition Root (main.rs)

Wires all `Arc<dyn Port>` adapters into `AppState`, then passes to `build_app()`.
Notable: `CachingProviderRegistry` decorates `PostgresProviderRegistry` (5s TTL) since `list_all()` runs on every job dequeue.

## Multi-Provider Routing (Intelligence Scheduler)

```
Client → POST /v1/chat/completions  (X-API-Key, source=Api)   → ZADD queue:zset (score=now_ms-tier_bonus)
      OR POST /v1/test/completions  (Bearer JWT, source=Test)  → ZADD queue:zset (score=now_ms-0)
       → queue_dispatcher_loop: ZRANGE peek top-K → Rust scoring → Lua ZREM claim → processing list
         → 3-stage model filter:
           0. global_model_settings → globally disabled? reject all
           1. providers_for_model() → has the model installed?
           2. list_enabled() → model enabled on this provider?
         → VRAM sort + model stickiness (+100GB bonus for loaded model)
         → tier sort (paid→non-free-tier, free→free-tier)
         → gate chain:
           circuit_breaker → thermal (per-provider, auto-detected GPU/CPU profile)
           → concurrency limit (AIMD-learned max_concurrent)
           → vram_pool.try_reserve() → VramPermit or skip to next in window
         → tokio::spawn run_job(permit)
           → ─ Phase 1 lifecycle (MCP_LIFECYCLE_PHASE=on):
                provider.ensure_ready(model) ← LlmProviderPort
                  warm hit / coalesce on LoadInFlight slot / cold-load probe
                  → updates VramPool SSOT, surfaces LifecycleError on fail
                  See flows/model-lifecycle.md.
              ─ Phase 2 inference:
                provider.stream_tokens(&job)
           → LlamaServerAdapter | GeminiAdapter → SSE tokens
           → permit dropped (auto) → KV cache returned, weight stays
           → ObservabilityPort → veronex-analytics → ClickHouse

ProcessManager + IdleManager (replaces legacy placement planner):
  • Lazy spawn: dispatcher calls `ensure_running` before claiming a permit;
    waits on a per-provider Mutex when a sibling caller is already spawning
  • TTL reap: `IdleManager.tick` (default 30s) stops processes idle past
    their TTL (default 60s, override per-provider via system_settings)
  • See `flows/process-manager.md`

Direct path (dev mode, no Valkey):
  pick_and_build() → gate chain → try_reserve() → None = skip (VRAM unavailable)

Reconnect:
  GET /v1/jobs/{id}/stream      (X-API-Key)  → SSE replay
  GET /v1/test/jobs/{id}/stream (Bearer JWT) → SSE replay

Background loops:
  health_checker (30s):
    → provider health (llama-server/Gemini)
    → hw_metrics fetch (node-exporter direct) → Valkey cache (HwMetrics with gpu_vendor)
    → thermal.set_thresholds(gpu_vendor) + thermal.update(temp_c)
    → infra service probes (postgresql/valkey/clickhouse/s3/vespa/embed) → veronex:svc:health:{instance_id} HASH
  idle_manager (default 30s tick):
    → ProcessManager.list_running() → stop providers idle >= TTL
    → AIMD per-model max_concurrent adjusted from live request outcomes
      (AimdController + SLO modules — no fleet-wide /api/tags scrape;
      llama-server has no /api/version, /api/tags, /api/ps, /api/show)
  per-node veronex-llm-agent:
    → reports probe (capacity, ports, drivers) + heartbeat
    → spawns / stops llama-server processes on ProcessManager request
```

## AppState

> Defined in `infrastructure/inbound/http/state.rs`. Field categories: `infra/deploy.md` -- AppState.
> All fields are `Arc<dyn Port>` -- wired in `main.rs` composition root.

## Message Bus

> Redpanda = single message bus. ClickHouse = read layer only (Kafka Engine → MV → MergeTree).
> Observability is fail-open: if unreachable, inference continues unrecorded.
> Full pipeline spec: `infra/otel-pipeline.md`.

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Queue-based LB | Veronex is the load balancer — no external LB needed |
| VRAM-aware routing | Minimizes model load cost (APU loads are slow) |
| GpuServer split | Multiple llama-server providers per host → single node-exporter scrape |
| SSE over WebSocket | Unidirectional stream is sufficient; simpler implementation |
| Arc<dyn Trait> | Runtime polymorphism; adapters freely swappable at composition root |
| async-trait kept | `Arc<dyn Port>` requires it; native async fn in trait is not dyn-safe |

## Port Catalog

### Inbound

| Port | Methods |
|------|---------|
| `InferenceUseCase` | `submit`, `process`, `stream`, `get_status`, `cancel` |

### Outbound

| Port | Adapter | Notes |
|------|---------|-------|
| `InferenceProviderPort` | `LlamaServerAdapter`, `GeminiAdapter` | Phase 2 — SSE streaming inference |
| `ModelLifecyclePort` | `LlamaServerAdapter`, `GeminiAdapter` (no-op) | Phase 1 — `ensure_ready` / `instance_state` / `evict` (Tier B) |
| `LlmProviderPort` (super-trait) | blanket impl over `InferenceProviderPort + ModelLifecyclePort` | Single trait object drives both phases (`make_adapter` returns `Arc<dyn LlmProviderPort>`) |
| `ProviderDispatchPort` | `ConcreteProviderDispatch` (carries `vram_pool`) | Provider selection, adapter build with `with_vram_pool`, Gemini rate-limit counters |
| `LlmProviderRegistry` | `CachingProviderRegistry` → `PostgresProviderRegistry` | 5s TTL decorator |
| `GpuServerRegistry` | `PostgresGpuServerRegistry` | Server + node-exporter |
| `JobRepository` | `PostgresJobRepository` | UPSERT on conflict |
| `ApiKeyRepository` | `PostgresApiKeyRepository` | BLAKE2b hash lookup |
| `ObservabilityPort` | `HttpObservabilityAdapter` | fail-open → veronex-analytics |
| `AuditPort` | `HttpAuditAdapter` | fail-open → veronex-analytics |
| `AnalyticsRepository` | `HttpAnalyticsClient` | GET from veronex-analytics |
| `AccountRepository` | `PostgresAccountRepository` | Argon2id, soft-delete, RBAC |
| `SessionRepository` | `PostgresSessionRepository` | jti + BLAKE2b refresh hash |
| `ModelCapacityRepository` | `PostgresModelCapacityRepository` | VRAM profiles (weight, KV, arch params) |
| `CapacitySettingsRepository` | `PostgresCapacitySettingsRepository` | Singleton (id=1) |
| `LlamaServerModelRepository` | `PostgresLlamaServerModelRepository` | Model-aware routing |
| `LlamaServerSyncJobRepository` | `PostgresLlamaServerSyncJobRepository` | Async sync (JSONB) |
| `GeminiPolicyRepository` | `PostgresGeminiPolicyRepository` | UPSERT + `*` fallback |
| `GeminiSyncConfigRepository` | `PostgresGeminiSyncConfigRepository` | Singleton admin key |
| `GeminiModelRepository` | `PostgresGeminiModelRepository` | Global model pool |
| `ProviderModelSelectionRepository` | `PostgresProviderModelSelectionRepository` | Per-provider model filter |
| `GlobalModelSettingsRepository` | `PostgresGlobalModelSettingsRepository` | Global model enable/disable (priority over per-provider) |
| `ApiKeyProviderAccessRepository` | `PostgresApiKeyProviderAccessRepository` | Per-key provider allow/deny |
| `VramPoolPort` | `VramPool`, `DistributedVramPool` | Per-provider VRAM pool: try_reserve → VramPermit (RAII, KV-only release) |
| `CircuitBreakerPort` | `CircuitBreakerMap` | Per-provider failure isolation (Closed→Open→HalfOpen) |
| `ThermalPort` | `ThermalThrottleMap` | Per-provider GPU thermal throttle level (Normal/Soft/Hard) |
| `LabSettingsRepository` | `PostgresLabSettingsRepository` | Feature flags (gemini_function_calling) |
| `ValkeyPort`             | `ValkeyAdapter`          | ZSET queue (enqueue/peek/claim/cancel), LIST legacy, KV, counters, pub/sub |
| `MessageStore` | `S3MessageStore` | MinIO/AWS S3 message storage |
| `ImageStore` | `S3ImageStore` | MinIO/AWS S3 image storage (WebP + thumbnails) |
