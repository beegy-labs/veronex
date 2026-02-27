# Hexagonal Architecture Policy

> SSOT | **Last Updated**: 2026-02-27
> Code patterns and templates → `policies/patterns.md`

## Overview

Veronex uses **Hexagonal Architecture (Ports & Adapters)** to isolate the LLM inference domain from infrastructure concerns (HTTP, Valkey, Postgres, OTel).

## Directory Structure

```
crates/inferq/src/
├── domain/
│   ├── entities/        # InferenceJob, LlmBackend, GpuServer, ApiKey, …
│   ├── enums/           # JobStatus, BackendType, LlmBackendStatus, …
│   └── value_objects/   # JobId, Prompt, ModelName
│
├── application/
│   ├── ports/
│   │   ├── inbound/     # InferenceUseCase (driving port)
│   │   └── outbound/    # all outbound port traits
│   └── use_cases/       # InferenceUseCaseImpl
│
├── infrastructure/
│   ├── inbound/http/    # Axum handlers, middleware, router, AppState, error.rs
│   └── outbound/
│       ├── persistence/ # Postgres adapters (one per port)
│       ├── ollama/      # OllamaAdapter
│       ├── gemini/      # GeminiAdapter
│       ├── backend_router.rs   # DynamicBackendRouter (VRAM-aware)
│       ├── health_checker.rs   # 30s background health checker
│       ├── model_manager.rs    # OllamaModelManager (LRU eviction)
│       └── observability/      # ClickHouseObservabilityAdapter
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

### Domain Layer

- No external dependencies, no async, no I/O
- Pure Rust structs and enums

### Application Layer

- Depends only on `domain`
- Defines all port traits using `#[async_trait]`
- Contains use case trait + impl

### Infrastructure Layer

- Implements port traits (adapters)
- Never contains business logic
- Depends on `application` to implement ports

## Composition Root (main.rs)

```rust
let pg_pool = database::connect(&database_url).await?;

let api_key_repo: Arc<dyn ApiKeyRepository> =
    Arc::new(PostgresApiKeyRepository::new(pg_pool.clone()));
let backend_registry: Arc<dyn LlmBackendRegistry> =
    Arc::new(PostgresBackendRegistry::new(pg_pool.clone()));
// ... repeat for every port

let state = AppState { use_case, api_key_repo, backend_registry, /* ... */ };
let app = build_app(state);
axum::serve(listener, app).await?;
```

## Multi-Backend Routing

```
Client → POST /v1/chat/completions  (OpenAI-compatible)
      OR POST /v1/inference          (native)
       → InferenceUseCaseImpl::submit → RPUSH veronex:queue:jobs
       → queue_dispatcher_loop (BLPOP)
         → DynamicBackendRouter::dispatch
           → list_active() → VRAM check → pick best backend
           → busy_backends.insert(id)     ← prevents double-dispatch
           → tokio::spawn run_job()
             → OllamaAdapter | GeminiAdapter → SSE tokens
             → ObservabilityPort::record_inference → ClickHouse
```

## AppState

```rust
// infrastructure/inbound/http/state.rs
pub struct AppState {
    pub use_case:                  Arc<dyn InferenceUseCase>,
    pub api_key_repo:              Arc<dyn ApiKeyRepository>,
    pub backend_registry:          Arc<dyn LlmBackendRegistry>,
    pub gpu_server_registry:       Arc<dyn GpuServerRegistry>,
    pub ollama_model_repo:         Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo:      Arc<dyn OllamaSyncJobRepository>,
    pub gemini_policy_repo:        Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo:   Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo:         Arc<dyn GeminiModelRepository>,
    pub model_selection_repo:      Arc<dyn BackendModelSelectionRepository>,
    pub valkey_pool:               Option<fred::clients::RedisPool>,
    pub clickhouse_client:         Option<clickhouse::Client>,
    pub pg_pool:                   sqlx::PgPool,
}
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Queue-based LB | Veronex is the load balancer — no external LB needed |
| VRAM-aware routing | Minimizes model load cost (APU loads are slow) |
| GpuServer split | Multiple Ollama backends per host → single node-exporter scrape |
| SSE over WebSocket | Unidirectional stream is sufficient; simpler implementation |
| Arc<dyn Trait> | Runtime polymorphism; adapters freely swappable at composition root |
| async-trait kept | `Arc<dyn Port>` requires it; native async fn in trait is not dyn-safe |

## Port Catalog

### Inbound

| Port | Methods |
|------|---------|
| `InferenceUseCase` | `submit`, `stream`, `get_status`, `cancel`, `recover_pending_jobs`, `start_queue_worker` |

### Outbound

| Port | Adapter | Notes |
|------|---------|-------|
| `InferenceBackendPort` | `OllamaAdapter`, `GeminiAdapter` | SSE streaming |
| `LlmBackendRegistry` | `PostgresBackendRegistry` | CRUD + health + update |
| `GpuServerRegistry` | `PostgresGpuServerRegistry` | Physical server + node-exporter |
| `JobRepository` | `PostgresJobRepository` | UPSERT on conflict |
| `ApiKeyRepository` | `PostgresApiKeyRepository` | BLAKE2b hash lookup |
| `ObservabilityPort` | `ClickHouseObservabilityAdapter` | inference_logs INSERT |
| `ModelManagerPort` | `OllamaModelManager` | LRU eviction, max_loaded=1 |
| `OllamaModelRepository` | `PostgresOllamaModelRepository` | Global Ollama model pool (model-aware routing) |
| `OllamaSyncJobRepository` | `PostgresOllamaSyncJobRepository` | Async sync job tracking (JSONB results) |
| `GeminiPolicyRepository` | `PostgresGeminiPolicyRepository` | UPSERT + `*` fallback |
| `GeminiSyncConfigRepository` | `PostgresGeminiSyncConfigRepository` | Singleton admin key |
| `GeminiModelRepository` | `PostgresGeminiModelRepository` | Global model pool |
| `BackendModelSelectionRepository` | `PostgresBackendModelSelectionRepository` | Per-paid-backend model filter |
