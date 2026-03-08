# Rust / Axum -- Graceful Shutdown Patterns

> **Last Researched**: 2026-03-02 | **Source**: tokio docs + tokio-util docs + web search
> **Status**: Verified -- planned for `crates/veronex/src/main.rs`
> **Parent**: See `rust-axum.md` for core Axum/Fred/sqlx patterns

---

## Current Pattern (this codebase -- fire-and-forget)

```rust
// main.rs -- no graceful shutdown mechanism
start_health_checker(registry.clone(), 30, valkey_pool.clone(), thermal.clone());
tokio::spawn(run_capacity_analysis_loop(...));
use_case_impl.start_queue_worker();  // spawns internally
axum::serve(listener, app).await?;   // process killed by OS on SIGTERM
```

**Problem:** On SIGTERM, tokio drops everything immediately. Background loops cannot flush state, drain the queue, or release semaphores.

---

## 2026 Best Practice -- `JoinSet` + `CancellationToken`

**Dependencies:**
- `JoinSet` -- in `tokio = "full"` (already available, since tokio 1.17)
- `CancellationToken` -- requires `tokio-util = { version = "0.7", features = ["rt"] }`

```rust
// main.rs -- graceful shutdown
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

---

## Loop Body -- `select!` for Cancellation

```rust
pub async fn run_health_checker_loop(
  registry: Arc<dyn LlmProviderRegistry>,
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

---

## BLPOP Loop -- Timeout is Already Cancellation-Friendly

BLPOP with a 5s timeout wakes up every 5 seconds naturally -- just check the token:

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

---

## `JoinSet` vs bare `tokio::spawn`

| | `JoinSet` | `tokio::spawn` (bare) |
|---|---|---|
| Task scoping | Scoped to parent lifetime | Detached, leaks on drop |
| Panic propagation | `JoinError` on `join_next()` | Silent abort |
| Graceful wait | `join_all()` / `join_next()` | Not possible |
| Use when | Long-running named loops | Fire-and-forget side effects |

---

## Graceful Shutdown Trigger Sources

Axum's `with_graceful_shutdown` accepts any `Future` -- common options:

```rust
// 1. OS signal (SIGTERM / Ctrl+C) -- use tokio::signal
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

## Sources

- tokio docs: https://docs.rs/tokio
- tokio-util CancellationToken: https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html
- Axum graceful shutdown: https://docs.rs/axum/latest/axum/serve/struct.Serve.html#method.with_graceful_shutdown
- Web search: tokio JoinSet graceful shutdown 2026, tokio-util CancellationToken patterns
- Verified: `crates/veronex/src/main.rs`
