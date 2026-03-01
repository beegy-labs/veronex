# Rust / Axum — 2026 Research

> **Last Researched**: 2026-03-02 | **Source**: Axum 0.8 docs + web search + verified in production
> **Status**: ✅ Verified — used throughout `crates/inferq/src/`

---

## Axum 0.8 — Critical Breaking Changes from 0.7

### Path Parameter Syntax

```rust
// ✅ Axum 0.8 — curly brace syntax
.route("/v1/servers/{id}/metrics", get(server_metrics_handler))

// ❌ Axum 0.7 and earlier — colon syntax (PANICS at startup in 0.8)
.route("/v1/servers/:id/metrics", get(server_metrics_handler))
```

**`:param` causes a startup panic in Axum 0.8.** All routes must use `{param}`.

---

## AppState Pattern

```rust
// State is cheaply cloneable (Arc-wrapped internals)
#[derive(Clone)]
pub struct AppState {
    pub pg_pool:     PgPool,
    pub valkey:      Arc<Client>,
    pub job_repo:    Arc<dyn JobRepository + Send + Sync>,
    // ... other repos/adapters
}

// Router construction
let app = Router::new()
    .route("/v1/...", get(handler))
    .with_state(state);

// Handler extraction
async fn handler(State(state): State<AppState>, ...) -> impl IntoResponse {
    // use state.job_repo, state.pg_pool, etc.
}
```

---

## Error Handling — AppError Pattern

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    // ...
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(m)  => (StatusCode::NOT_FOUND, m.clone()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            AppError::Database(_)  => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
```

---

## SSE Streaming

```rust
use axum::response::sse::{Event, Sse};
use tokio_stream::StreamExt;

async fn stream_handler(State(state): State<AppState>, ...) -> Sse<impl Stream<Item=...>> {
    let stream = async_stream::stream! {
        loop {
            let chunk = receiver.recv().await;
            yield Ok(Event::default().data(chunk));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

---

## Middleware — from_fn

```rust
// Auth middleware via from_fn
pub async fn require_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let key = extract_api_key(&req)?;
    let api_key = state.key_repo.find_by_hash(&key).await?
        .ok_or(AppError::Unauthorized)?;
    // inject into request extensions
    let mut req = req;
    req.extensions_mut().insert(api_key);
    Ok(next.run(req).await)
}
```

---

## tokio — Background Task Patterns

### Current Pattern (this codebase — fire-and-forget)

```rust
// main.rs — no graceful shutdown mechanism
start_health_checker(registry.clone(), 30, valkey_pool.clone(), thermal.clone());
tokio::spawn(run_capacity_analysis_loop(...));
use_case_impl.start_queue_worker();  // spawns internally
axum::serve(listener, app).await?;   // process killed by OS on SIGTERM
```

**Problem:** On SIGTERM, tokio drops everything immediately. Background loops cannot flush state, drain the queue, or release semaphores.

### 2026 Best Practice — `JoinSet` + `CancellationToken`

**Dependencies:**
- `JoinSet` — in `tokio = "full"` (already available, since tokio 1.17)
- `CancellationToken` — requires `tokio-util = { version = "0.7", features = ["rt"] }`

```rust
// main.rs — graceful shutdown
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

let shutdown = CancellationToken::new();
let mut tasks = JoinSet::new();

// All background tasks share the same shutdown signal
tasks.spawn(run_health_checker_loop(..., shutdown.child_token()));
tasks.spawn(run_capacity_analysis_loop(..., shutdown.child_token()));
tasks.spawn(run_queue_dispatcher_loop(..., shutdown.child_token()));

// Axum drains in-flight requests, then signals shutdown
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown.clone().cancelled_owned())
    .await?;

shutdown.cancel();  // propagate to all child tokens

// Wait for each loop to acknowledge shutdown
while let Some(result) = tasks.join_next().await {
    if let Err(e) = result {
        tracing::warn!("background task panicked: {e}");
    }
}
```

### Loop Body — `select!` for Cancellation

```rust
pub async fn run_health_checker_loop(
    registry: Arc<dyn LlmBackendRegistry>,
    interval_secs: u64,
    thermal: Arc<ThermalThrottleMap>,
    shutdown: CancellationToken,
) {
    let interval = Duration::from_secs(interval_secs);
    loop {
        tokio::select! {
            biased;               // check shutdown first (higher priority)
            _ = shutdown.cancelled() => {
                tracing::info!("health checker: shutdown signal received");
                break;
            }
            _ = tokio::time::sleep(interval) => {
                // health check logic...
            }
        }
    }
}
```

### BLPOP Loop — Timeout is Already Cancellation-Friendly

BLPOP with a 5s timeout wakes up every 5 seconds naturally — just check the token:

```rust
loop {
    tokio::select! {
        biased;
        _ = shutdown.cancelled() => {
            tracing::info!("queue dispatcher: draining in-flight jobs then shutting down");
            break;
        }
        result = blpop(&pool, &[QUEUE_KEY_API, QUEUE_KEY_TEST], 5.0) => {
            // process job...
        }
    }
}
```

### `JoinSet` vs bare `tokio::spawn`

| | `JoinSet` | `tokio::spawn` (bare) |
|---|---|---|
| Task scoping | ✅ Scoped to parent lifetime | ❌ Detached, leaks on drop |
| Panic propagation | ✅ `JoinError` on `join_next()` | ❌ Silent abort |
| Graceful wait | ✅ `join_all()` / `join_next()` | ❌ Not possible |
| Use when | Long-running named loops | Fire-and-forget side effects |

### Graceful Shutdown Trigger Sources

Axum's `with_graceful_shutdown` accepts any `Future` — common options:

```rust
// 1. OS signal (SIGTERM / Ctrl+C) — use tokio::signal
async fn shutdown_signal() {
    let ctrl_c = async { signal(SignalKind::interrupt())?.recv().await };
    let terminate = async { signal(SignalKind::terminate())?.recv().await };
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;

// 2. CancellationToken (preferred when coordinating with JoinSet)
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_token.cancelled_owned())
    .await?;
```

---

## sqlx — Query Patterns

```rust
// query_as! macro — compile-time checked
let job = sqlx::query_as!(
    InferenceJob,
    "SELECT id, model_name, status, ... FROM jobs WHERE id = $1",
    job_id
)
.fetch_optional(&pool)
.await?;

// UPSERT pattern
sqlx::query!(
    "INSERT INTO jobs (...) VALUES (...) ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status",
    ...
)
.execute(&pool)
.await?;
```

---

## Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| `:param` in routes (Axum 0.8) | Startup panic | Use `{param}` |
| `unwrap()` in handlers | 500 with no context | `?` operator + `AppError` |
| Blocking call in async fn | Starves tokio runtime | `tokio::task::spawn_blocking(|| ...)` |
| `Arc<Mutex<>>` around DB pool | PgPool is already Arc | Use `PgPool` directly |
| `.clone()` large state frequently | Heap allocation per request | Ensure inner data is Arc-wrapped |
| `tokio::spawn` for long-running loops | No graceful shutdown, silent panics | `JoinSet` + `CancellationToken` |
| No `with_graceful_shutdown` | In-flight requests cut mid-stream | `axum::serve(...).with_graceful_shutdown(token.cancelled_owned())` |
| `onSuccess` for cache invalidation | Skips invalidation on error | Use `onSettled` |

---

## Sources

- Axum 0.8 changelog: https://github.com/tokio-rs/axum/releases
- tokio docs: https://docs.rs/tokio
- tokio-util CancellationToken: https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html
- Axum graceful shutdown: https://docs.rs/axum/latest/axum/serve/struct.Serve.html#method.with_graceful_shutdown
- sqlx docs: https://docs.rs/sqlx
- Web search: tokio JoinSet graceful shutdown 2026, tokio-util CancellationToken patterns
- Verified: `crates/inferq/src/infrastructure/inbound/http/`, `crates/inferq/src/main.rs`
