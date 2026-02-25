# Hexagonal Architecture Policy

> SSOT for inferq architecture | **Last Updated**: 2026-02-25

## Overview

inferq uses **Hexagonal Architecture (Ports & Adapters)** to isolate the LLM inference domain from infrastructure concerns (HTTP, Valkey, Postgres, OTel).

## Directory Structure

```
crates/inferq/src/
├── domain/
│   ├── entities/        # InferenceJob, LlmBackend, GpuServer, ApiKey, Model, …
│   ├── enums/           # JobStatus, BackendType, LlmBackendStatus, …
│   └── value_objects/   # JobId, Prompt, ModelName
│
├── application/
│   ├── ports/
│   │   ├── inbound/     # InferenceUseCase (driving port)
│   │   └── outbound/    # LlmBackendRegistry, GpuServerRegistry, JobRepository,
│   │                    # ApiKeyRepository, ObservabilityPort, ModelManagerPort, …
│   └── use_cases/       # InferenceUseCaseImpl
│
├── infrastructure/
│   ├── inbound/
│   │   └── http/        # Axum handlers, middleware, router, AppState
│   └── outbound/
│       ├── persistence/ # PostgresBackendRegistry, PostgresGpuServerRegistry,
│       │                # PostgresJobRepository, PostgresApiKeyRepository
│       ├── ollama/      # OllamaAdapter (InferenceBackendPort)
│       ├── gemini/      # GeminiAdapter (InferenceBackendPort)
│       ├── backend_router.rs  # DynamicBackendRouter (VRAM-aware dispatch)
│       ├── health_checker.rs  # 30s background health checker
│       ├── model_manager.rs   # OllamaModelManager (LRU eviction)
│       └── observability/     # ClickHouseObservabilityAdapter
│
└── main.rs              # Composition root — wires all adapters into AppState
```

## Layers

### Domain Layer

- No external dependencies (no async, no I/O)
- Pure Rust structs / enums
- Contains: entities, value objects, domain enums

```rust
// domain/entities/mod.rs
pub struct InferenceJob {
    pub id: JobId,
    pub prompt: Prompt,
    pub model_name: ModelName,
    pub status: JobStatus,
    pub backend: BackendType,
    pub created_at: DateTime<Utc>,
    // …
}

pub struct GpuServer {
    pub id: Uuid,
    pub name: String,
    pub node_exporter_url: Option<String>, // live metrics fetched from here
    pub registered_at: DateTime<Utc>,
}

pub struct LlmBackend {
    pub id: Uuid,
    pub server_id: Option<Uuid>,   // FK → GpuServer
    pub gpu_index: Option<i16>,
    pub total_vram_mb: i64,
    // …
}
```

### Application Layer

- Depends only on domain
- Defines all ports as Rust traits
- Contains: use case trait + impl, all port traits

```rust
// application/ports/outbound/gpu_server_registry.rs
#[async_trait]
pub trait GpuServerRegistry: Send + Sync {
    async fn register(&self, server: GpuServer) -> Result<()>;
    async fn list_all(&self) -> Result<Vec<GpuServer>>;
    async fn get(&self, id: Uuid) -> Result<Option<GpuServer>>;
    async fn delete(&self, id: Uuid) -> Result<()>;
}

// application/ports/inbound/inference_use_case.rs
#[async_trait]
pub trait InferenceUseCase: Send + Sync {
    async fn submit(&self, req: InferenceRequest) -> Result<JobId>;
    async fn stream(&self, job_id: JobId) -> Result<impl Stream<Item = String>>;
    async fn get_status(&self, job_id: &JobId) -> Result<JobStatus>;
    async fn cancel(&self, job_id: &JobId) -> Result<()>;
}
```

### Infrastructure Layer

- Depends on application (implements port traits)
- Never contains business logic
- Adapters are concrete structs that `impl SomePort`

```rust
// infrastructure/outbound/persistence/gpu_server_registry.rs
pub struct PostgresGpuServerRegistry { pool: PgPool }

#[async_trait]
impl GpuServerRegistry for PostgresGpuServerRegistry {
    async fn register(&self, server: GpuServer) -> Result<()> { … }
    async fn list_all(&self) -> Result<Vec<GpuServer>> { … }
    // …
}
```

## Dependency Rule

```
infrastructure → application → domain
```

- `domain` imports nothing from other layers
- `application` imports only from `domain`
- `infrastructure` imports from `application` (to implement ports)

**Violation**: Any reverse dependency is a compile error (Rust enforces this naturally).

## Composition Root

All wiring happens in `main.rs`:

```rust
// main.rs
let pg_pool = database::connect(&database_url).await?;
let backend_registry: Arc<dyn LlmBackendRegistry> =
    Arc::new(PostgresBackendRegistry::new(pg_pool.clone()));
let gpu_server_registry: Arc<dyn GpuServerRegistry> =
    Arc::new(PostgresGpuServerRegistry::new(pg_pool.clone()));

let state = AppState {
    use_case,
    api_key_repo,
    backend_registry,
    gpu_server_registry,
    valkey_pool,
    clickhouse_client,
    pg_pool,
};

let app = build_app(state);
axum::serve(listener, app).await?;
```

## Multi-Backend Routing

```
Client → POST /v1/inference
       → InferenceUseCaseImpl::submit → enqueue (Valkey RPUSH)
       → queue_dispatcher_loop (BLPOP)
         → DynamicBackendRouter::dispatch
           → list_active() → VRAM check (/api/ps) → pick best
           → busy_backends.insert(id)   ← prevents double-dispatch
           → tokio::spawn run_job()
             → OllamaAdapter | GeminiAdapter
             → ObservabilityPort::record_inference
             → stream buffer → SSE
```

## AppState (Shared State)

```rust
// infrastructure/inbound/http/state.rs
pub struct AppState {
    pub use_case: Arc<dyn InferenceUseCase>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub backend_registry: Arc<dyn LlmBackendRegistry>,
    pub gpu_server_registry: Arc<dyn GpuServerRegistry>,
    pub valkey_pool: Option<fred::clients::RedisPool>,
    pub clickhouse_client: Option<clickhouse::Client>,
    pub pg_pool: sqlx::PgPool,
}
```

## Key Design Decisions

| Decision | Rationale |
| -------- | --------- |
| Queue + LB 통합 | inferq 자체가 LB — 외부 LB 불필요 |
| VRAM-aware routing | 모델 로드 비용 최소화 (APU는 로드가 느림) |
| GpuServer 분리 | 동일 호스트 N개 Ollama 백엔드 → node-exporter 중복 scrape 방지 |
| SSE over WebSocket | Unidirectional stream 충분; 구현 단순 |
| Port per concern | 독립 테스트 가능, 어댑터 교체 용이 |
| Arc<dyn Trait> | 런타임 다형성으로 조합 루트에서 자유로운 배선 |

## Port Catalog

### Inbound (Driving)

| Port | Methods |
| ---- | ------- |
| `InferenceUseCase` | `submit`, `stream`, `get_status`, `cancel`, `recover_pending_jobs`, `start_queue_worker` |

### Outbound (Driven)

| Port | Adapter | Notes |
| ---- | ------- | ----- |
| `InferenceBackendPort` | `OllamaAdapter`, `GeminiAdapter` | SSE streaming |
| `LlmBackendRegistry` | `PostgresBackendRegistry` | CRUD + health status + update |
| `GpuServerRegistry` | `PostgresGpuServerRegistry` | Physical server + node-exporter |
| `JobRepository` | `PostgresJobRepository` | UPSERT on conflict |
| `ApiKeyRepository` | `PostgresApiKeyRepository` | Hash-based lookup |
| `ObservabilityPort` | `ClickHouseObservabilityAdapter` | inference_logs INSERT |
| `ModelManagerPort` | `OllamaModelManager` | LRU eviction, max_loaded=1 |
