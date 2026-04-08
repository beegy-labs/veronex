# Rust / Axum -- 2026 Research

> **Last Researched**: 2026-03-02 | **Source**: Axum 0.8 docs + web search + verified in production
> **Status**: Verified -- used throughout `crates/veronex/src/`
> **Deps**: fred 10 (`Pool` type, `pool.init().await?`), reqwest 0.13
> **Companion**: `rust-axum-shutdown.md` (tokio graceful shutdown) | `rust-axum-2026.md` (2026 additions)

---

## Fred 10 -- Valkey/Redis Client

Fred 10 renamed core types. All `use fred::prelude::*;` imports continue to work; only the type names changed.

| Fred 9 | Fred 10 | Notes |
|--------|---------|-------|
| `RedisConfig` | `Config` | from `fred::prelude::*` |
| `RedisPool` | `Pool` | from `fred::clients::Pool` |
| `RedisClient` | `Client` | from `fred::clients::Client` |
| `pool.connect(); pool.wait_for_connect().await?` | `pool.init().await?` | Convenience: connect + wait in one call |

```rust
// fred 10 -- Pool init pattern
use fred::prelude::*;
let config = Config::from_url(&valkey_url)?;
let pool = Pool::new(config, None, None, None, 6)?;
pool.init().await?;
```

- `Expiration::EX(secs)` -- unchanged, still in `fred::types::Expiration`
- `pool.blpop(keys, timeout)`, `pool.set(...)`, `pool.get(...)`, `pool.exists(...)` -- signatures unchanged
- `pool.next()` returns `Arc<Client>` (was `Arc<RedisClient>`)
- `use fred::interfaces::LuaInterface as _` -- unchanged

---

## Axum 0.8 -- Critical Breaking Changes from 0.7

### Path Parameter Syntax

```rust
// Axum 0.8 -- curly brace syntax
.route("/v1/servers/{id}/metrics", get(server_metrics_handler))

// Axum 0.7 and earlier -- colon syntax (PANICS at startup in 0.8)
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

## Error Handling -- AppError Pattern

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

## Middleware -- from_fn

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

## sqlx -- Query Patterns

```rust
// query_as! macro -- compile-time checked
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
| Blocking call in async fn | Starves tokio runtime | `tokio::task::spawn_blocking(\|\| ...)` |
| `Arc<Mutex<>>` around DB pool | PgPool is already Arc | Use `PgPool` directly |
| `.clone()` large state frequently | Heap allocation per request | Ensure inner data is Arc-wrapped |
| `tokio::spawn` for long-running loops | No graceful shutdown, silent panics | `JoinSet` + `CancellationToken` (see `rust-axum-shutdown.md`) |
| No `with_graceful_shutdown` | In-flight requests cut mid-stream | See `rust-axum-shutdown.md` |
| `onSuccess` for cache invalidation | Skips invalidation on error | Use `onSettled` |

---

## Sources

- [Announcing axum 0.8.0 — Tokio Blog](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0)
- [axum CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md)
- tokio docs: https://docs.rs/tokio | sqlx docs: https://docs.rs/sqlx
- Verified: `crates/veronex/src/infrastructure/inbound/http/`, `crates/veronex/src/main.rs`
