# Code Patterns — 2026 Reference

> SSOT | **Last Updated**: 2026-03-02 (rev: DashMap concurrent map pattern; Lua eval atomic Valkey ops)
> Rust Edition 2024 · Axum 0.8 · sqlx 0.8 · Next.js 16 · React 19 · TanStack Query v5

---

## Rust: Axum 0.8 Handler Signature

Every handler follows this exact signature pattern:

```rust
// Read — returns single resource
pub async fn get_thing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ThingSummary>, AppError> {
    let thing = state.thing_repo.get(id).await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(to_summary(&thing)))
}

// Create — returns 201 + body
pub async fn create_thing(
    State(state): State<AppState>,
    Json(req): Json<CreateThingRequest>,
) -> Result<(StatusCode, Json<ThingSummary>), AppError> {
    let thing = state.thing_repo.create(req.into()).await?;
    Ok((StatusCode::CREATED, Json(to_summary(&thing))))
}

// Delete — returns 204 No Content
pub async fn delete_thing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.thing_repo.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

---

## Rust: Error Handling — AppError (thiserror v2)

2026 standard: define domain errors with `thiserror` → implement `IntoResponse` → handlers use `?` cleanly.

> `thiserror = "2"` is already in `Cargo.toml` but not yet fully adopted. All new handlers should use this pattern.

```rust
// infrastructure/inbound/http/error.rs  ← create this file
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            Self::NotFound      => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            Self::Unauthorized  => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::Internal(e)   => {
                tracing::error!("internal: {e:#}"); // preserve context, hide from client
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".into())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

// Optional type alias
pub type ApiResult<T> = Result<Json<T>, AppError>;
```

**Current codebase**: handlers use `impl IntoResponse` + manual `StatusCode` tuples (~50 repetitions).
To migrate: create `error.rs` above → change handler return types to `Result<T, AppError>`.

---

## Rust: sqlx — Compile-Time SQL Verification

```rust
// ✅ Recommended: query_as! + FromRow
// Requires DATABASE_URL in .env at compile time
#[derive(sqlx::FromRow)]
struct BackendRow {
    id: Uuid,
    name: String,
    backend_type: String,
    // ... one field per DB column
}

let row = sqlx::query_as!(
    BackendRow,
    "SELECT id, name, backend_type FROM llm_backends WHERE id = $1",
    id
)
.fetch_optional(&self.pool)
.await?;

// ⚠️ Never use SELECT * — column order breaks with JOINs

// Current codebase: uses query() + manual row_to_entity() mapping.
// New repositories should use query_as! pattern.
```

---

## Rust: async-trait (Required — Do Not Remove)

```rust
// #[async_trait] is STILL required for Arc<dyn Trait> (trait objects)
// Rust 1.75+ async fn in trait is only object-safe with `impl Trait`, not `dyn Trait`
// This project uses Arc<dyn ApiKeyRepository> → keep #[async_trait]

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn get_by_hash(&self, hash: &str) -> anyhow::Result<Option<ApiKey>>;
}

// ❌ Removing #[async_trait] breaks Arc<dyn ApiKeyRepository> at compile time
```

---

## Rust: tracing + OpenTelemetry

2026 standard: `tracing` crate is the de facto Rust instrumentation standard. Combine with OTel for distributed traces.

```rust
use tracing::{info, error, instrument};

// Add #[instrument] to important handlers and background tasks
#[instrument(skip(state), fields(backend_id = %id))]
pub async fn get_backend(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<BackendSummary>, AppError> {
    info!("fetching backend");
    let b = state.backend_registry.get(id).await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(to_summary(&b)))
}

// Propagate span into spawned tasks
let span = tracing::info_span!("run_job", job_id = %job_id);
tokio::spawn(async move { run_job(state, job_id).await }.instrument(span));

// OTEL_EXPORTER_OTLP_ENDPOINT env → enables gRPC exporter → traces to ClickHouse
```

---

## Rust: Concurrent Maps — DashMap (not `Mutex<HashMap>`)

Use `dashmap::DashMap` for any map shared across async tasks.
`dashmap = "6"` is already in `Cargo.toml`.

```rust
// ✅ DashMap — 64 shards, different keys never contend
use dashmap::DashMap;
let jobs: Arc<DashMap<Uuid, JobEntry>> = Arc::new(DashMap::new());

// Insert — no lock, no await
jobs.insert(id, entry);

// Read — returns Ref<'_, K, V>, must be dropped before .await
let value = jobs.get(&id).map(|r| r.clone());  // clone what you need

// Mutate — RefMut must be dropped before .await or notify calls
let notify = {
    let mut entry = jobs.get_mut(&id).ok_or(NotFound)?;
    entry.tokens.push(token);
    entry.notify.clone()                        // clone before drop
};                                              // RefMut dropped here ← critical
notify.notify_one();
```

**Rule**: never hold a `Ref`/`RefMut` across an `.await` point or a `yield` — it locks the shard.

```rust
// ❌ Wrong — RefMut alive across notify (which internally .awaits on Notify::notified)
let mut entry = jobs.get_mut(&id)?;
entry.tokens.push(token);
entry.notify.notify_one();
some_async_fn().await;   // shard still locked
```

`std::sync::Mutex<HashMap>` serialises all concurrent accesses on a single lock — avoid it
for maps used in async hot paths. DashMap shards by key hash, so independent keys never block.

---

## Rust: Atomic Valkey/Redis Ops — Lua Eval

Multi-step Valkey operations (read-modify-write) must be atomic to avoid races.
Use a single `EVAL` instead of multiple round-trips.

`fred = "9"` requires feature `i-scripts` for `LuaInterface`:

```toml
# Cargo.toml
fred = { version = "9", features = ["serde-json", "i-scripts"] }
```

```rust
use fred::interfaces::LuaInterface as _;

// Sliding-window RPM check: 4 commands → 1 atomic round-trip
const RATE_LIMIT_SCRIPT: &str = r#"
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('EXPIRE', KEYS[1], 62)
return redis.call('ZCARD', KEYS[1])
"#;

// pool.next() → Arc<RedisClient> which implements LuaInterface
let count: u64 = pool
    .next()
    .eval(
        RATE_LIMIT_SCRIPT,
        vec![key.to_string()],
        vec![window_start.to_string(), now_ms.to_string(), member.to_string()],
    )
    .await?;
```

- `pool.next()` round-robins across pool clients; it does **not** pin to a slot.
- Lua scripts run atomically on the Valkey server — no interleaved commands from other clients.
- Fail-open pattern: wrap in `match` and log on error, let the request through.

---

## Rust: Background Tasks — JoinSet + CancellationToken

> Full research: `research/backend/rust-axum.md` (Background Tasks section)

### Current State (this codebase)

Three background loops are launched as fire-and-forget `tokio::spawn`:

```rust
// main.rs — current pattern (no graceful shutdown)
start_health_checker(registry.clone(), 30, valkey_pool.clone(), thermal.clone());
tokio::spawn(run_capacity_analysis_loop(...));
use_case_impl.start_queue_worker();   // spawns internally
axum::serve(listener, app).await?;   // process dies on SIGTERM/Ctrl+C
```

**Problem:** On SIGTERM, tokio drops the runtime immediately. Background tasks cannot flush in-flight state, drain the queue, or release locks.

### 2026 Best Practice — `JoinSet` + `CancellationToken`

`JoinSet` (tokio 1.17+, already in `tokio = "full"`) scopes spawned tasks.
`CancellationToken` (requires adding `tokio-util`) propagates shutdown signals.

```rust
// Cargo.toml — add:
// tokio-util = { version = "0.7", features = ["rt"] }

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let shutdown = CancellationToken::new();
    let mut tasks = JoinSet::new();

    // Pass child tokens — cancel() on parent propagates to all children
    tasks.spawn(run_health_checker_loop(
        registry.clone(), 30, valkey_pool.clone(), thermal.clone(),
        shutdown.child_token(),
    ));
    tasks.spawn(run_capacity_analysis_loop(
        ..., shutdown.child_token(),
    ));
    tasks.spawn(run_queue_dispatcher_loop(
        ..., shutdown.child_token(),
    ));

    // Axum graceful shutdown — waits for in-flight requests to drain
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown.clone().cancelled_owned())
        .await?;

    // Signal all background tasks to stop
    shutdown.cancel();

    // Wait for all loops to exit cleanly
    while let Some(res) = tasks.join_next().await {
        if let Err(e) = res { tracing::warn!("background task panicked: {e}"); }
    }
    Ok(())
}
```

### Loop Signature Convention

Each background loop accepts a `CancellationToken` and uses `select!` to exit cleanly:

```rust
pub async fn run_health_checker_loop(
    registry: Arc<dyn LlmBackendRegistry>,
    interval_secs: u64,
    valkey_pool: Option<Pool>,
    thermal: Arc<ThermalThrottleMap>,
    shutdown: CancellationToken,          // ← added parameter
) {
    let interval = Duration::from_secs(interval_secs);
    tracing::info!("health checker started");

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                tracing::info!("health checker shutting down");
                break;
            }
            _ = tokio::time::sleep(interval) => {
                // run health check cycle...
            }
        }
    }
}
```

### BLPOP Loop (queue dispatcher) — select! pattern

BLPOP with a timeout is already cancellation-friendly:

```rust
loop {
    tokio::select! {
        _ = shutdown.cancelled() => { break; }
        result = blpop(&pool, &["queue:jobs", "queue:test"], 5.0) => {
            // process job...
        }
    }
}
```

### Migration Plan (Phase 4)

1. Add `tokio-util = { version = "0.7", features = ["rt"] }` to `Cargo.toml`
2. Refactor `start_health_checker` → `run_health_checker_loop(shutdown: CancellationToken)`
3. Add `shutdown: CancellationToken` parameter to `run_capacity_analysis_loop`
4. Refactor `start_queue_worker` / `queue_dispatcher_loop` → accept `CancellationToken`
5. Update `main.rs`: `JoinSet` + `axum::serve(...).with_graceful_shutdown(...)`

---

## Rust: sqlx — Pool Configuration for Production

```rust
// database.rs — production-ready pool options
use sqlx::postgres::PgPoolOptions;

pub async fn connect(url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(10)                           // default: 10
        .min_connections(2)                            // keep 2 warm connections
        .acquire_timeout(Duration::from_secs(5))       // fail fast on pool exhaustion
        .idle_timeout(Duration::from_secs(600))        // release idle after 10m
        .max_lifetime(Duration::from_secs(1800))       // recycle connections every 30m
        .connect(url)
        .await
        .context("failed to connect to postgres")
}
```

**Current codebase:** `database::connect()` uses default pool options. Explicit settings prevent connection pool exhaustion under load.

On shutdown:
```rust
pg_pool.close().await;  // drain pool gracefully before process exits
```

---

## Rust: Adding a New Port + Adapter

Strict order to respect hexagonal dependency rule:

```
1. domain/entities/new_entity.rs              ← pure struct, no I/O
2. application/ports/outbound/new_port.rs     ← #[async_trait] trait; add to mod.rs
3. migrations/YYYYMMDDHHMMSS_description.sql  ← DB migration
4. infrastructure/outbound/persistence/new.rs ← impl the trait; add to mod.rs
5. infrastructure/inbound/http/state.rs       ← add Arc<dyn NewPort> field
6. main.rs                                    ← init + inject into AppState
7. infrastructure/inbound/http/new_handlers.rs ← use Result<T, AppError>
8. infrastructure/inbound/http/router.rs      ← register routes inside auth middleware
9. docs/llm/backend/new_feature.md            ← CDD doc
```

---

## Frontend: TanStack Query v5

> Full research: `research/frontend/tanstack-query.md`

### `queryOptions()` Factory — SSOT Pattern (2026)

Define query configuration **once** in `web/lib/queries/` and reuse across components:

```typescript
// web/lib/queries/dashboard.ts
import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

export const dashboardStatsQuery = queryOptions({
  queryKey: ['dashboard', 'stats'],
  queryFn: () => api.stats(),
  staleTime: 30_000,
  retry: false,
})

export const jobsQuery = (params?: string) => queryOptions({
  queryKey: ['dashboard', 'jobs', params],
  queryFn: () => api.jobs(params),
  staleTime: 4_900,
  refetchInterval: 5_000,
  refetchIntervalInBackground: false,
})
```

```typescript
// In a page component — use the factory
const { data } = useQuery(dashboardStatsQuery)
const { data: jobs } = useQuery(jobsQuery('status=completed'))
```

Benefits: single place to change staleTime/retry, type-safe key sharing, reuse in `prefetchQuery`.

### Inline `useQuery` (fallback for one-off, modal-only fetches)

```typescript
// Conditional fetch — only when prerequisites are met (modal, etc.)
const { data } = useQuery({
  queryKey: ['job-detail', jobId],
  queryFn: () => api.jobDetail(jobId!),
  enabled: !!jobId && open,   // fetch only when modal is open
})
```

### Mutation — use `onSettled` for cache invalidation

```typescript
// CORRECT — onSettled runs on both success and error
const mutation = useMutation({
  mutationFn: (id: string) => api.deleteBackend(id),
  onSettled: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  onError: (e: Error) => console.error(e.message),
})
mutation.mutate(id)            // fire-and-forget
await mutation.mutateAsync(id) // await inside async handler

// WRONG — onSuccess skips invalidation on error (stale UI)
onSuccess: () => queryClient.invalidateQueries(...)
```

---

## Frontend: React 19 — useOptimistic

2026 standard: apply optimistic updates to all toggle/switch mutations for perceived speed.

```typescript
import { useOptimistic } from 'react'

// useOptimistic(currentValue, updater)
const [optimisticEnabled, setOptimistic] = useOptimistic(
  model.is_enabled,
  (_, newValue: boolean) => newValue
)

const mutation = useMutation({
  mutationFn: (v: boolean) => api.setModelEnabled(backendId, model.model_name, v),
  onError: () => setOptimistic(model.is_enabled), // auto-revert on failure
})

<Switch
  checked={optimisticEnabled}
  onCheckedChange={(v) => { setOptimistic(v); mutation.mutate(v) }}
/>
// UI responds instantly → server syncs in background → reverts if error
```

---

## Frontend: TypeScript + Zod (API Boundary Validation)

2026 standard: TypeScript enforces compile-time types; Zod validates untrusted API responses at runtime.

```typescript
// web/lib/types.ts
import { z } from 'zod'

// Define schema first, infer type from it
export const BackendSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  backend_type: z.enum(['ollama', 'gemini']),
  status: z.enum(['online', 'offline', 'degraded']),
  is_active: z.boolean(),
})
export type Backend = z.infer<typeof BackendSchema>

// Use safeParse to handle errors gracefully (no throws)
const result = BackendSchema.safeParse(apiResponse)
if (!result.success) console.error(result.error.issues)

// Branded types prevent wrong-ID bugs
const BackendIdSchema = z.string().uuid().brand<'BackendId'>()
type BackendId = z.infer<typeof BackendIdSchema>
```

Apply Zod at entry points: API responses, form inputs, env vars.

---

## Frontend: Tailwind v4 Color Rules

```tsx
// ✅ Use @theme-generated utilities (from tokens.css @theme inline block)
<div className="bg-bg-card text-text-primary border border-border rounded-md p-4">

// ✅ Inline dynamic values via CSS vars
<span style={{ color: 'var(--theme-text-secondary)' }}>

// ✅ Status colors (per design spec, both modes)
const STATUS_COLOR: Record<JobStatus, string> = {
  completed: 'text-emerald-400',  // #34d399
  failed:    'text-rose-400',     // #fb7185
  pending:   'text-amber-400',    // #fbbf24
  running:   'text-blue-400',     // #60a5fa
  cancelled: 'text-slate-400',
}

// ❌ Never: hardcoded hex in style prop
// ❌ Never: non-theme Tailwind color classes (text-slate-700 etc.)
```

---

## Frontend: Adding a New Page

```
1. web/lib/types.ts               ← add TypeScript types (+ Zod schema if untrusted data)
2. web/lib/api.ts                 ← add API functions to the api object
3. web/lib/queries/domain.ts      ← add queryOptions factory (SSOT for queryKey + staleTime)
4. web/app/new-page/page.tsx      ← 'use client' + useQuery(domainQuery) + UI
5. web/components/nav.tsx         ← add navItems entry
6. web/messages/en.json           ← add i18n keys (source of truth)
7. web/messages/ko.json           ← Korean translation
8. web/messages/ja.json           ← Japanese translation
9. docs/llm/frontend/web-*.md     ← update CDD doc
```
