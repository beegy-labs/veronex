# Rust / Axum — 2026 Updates

> **Last Researched**: 2026-04-07 | **Source**: Axum changelog, tokio docs, sqlx docs, web search
> **Companion**: `research/backend/rust-axum.md` — core patterns

---

## Axum 0.8.x — New Capabilities

| Feature | Detail |
|---------|--------|
| `ListenerExt::limit_connections(n)` | Hard cap on concurrent connections — use before `axum::serve()` |
| `NoContent` type | Shorthand for `StatusCode::NO_CONTENT` response |
| JSON trailing garbage rejection | `Json` extractor now rejects bytes after the JSON document |
| `serde_path_to_error` in Query/Form | Validation errors include the field path automatically |
| Path tuple param count validation | Startup panic if handler param count mismatches route |
| `method_not_allowed_fallback` | Custom 405 handler per router |
| HTTP/2 WebSocket | Use `any()` instead of `get()` for the route |

`#[async_trait]` is **removed** from `FromRequest`/`FromRequestParts` — RPITIT is stable Rust now. Remove from all custom extractors.

---

## sqlx 0.8 — Pool Tuning

```rust
let pool = PgPoolOptions::new()
    .max_connections(20)                           // (CPU cores × 2) + 1, cap at 20
    .min_connections(2)                            // warm pool — skips cold-start latency
    .acquire_timeout(Duration::from_secs(5))       // fail fast on pool exhaustion
    .idle_timeout(Some(Duration::from_secs(600)))  // 10 min idle before recycle
    .max_lifetime(Some(Duration::from_secs(1800))) // 30 min max conn age
    .statement_cache_capacity(256)                 // reduces parse/plan overhead
    .connect(&database_url)
    .await?;
```

**Offline CI:** Run `cargo sqlx prepare -- --lib`, commit `.sqlx/` dir, set `SQLX_OFFLINE=true` in CI.

---

## Tokio — Async Performance Rules

| Rule | Detail |
|------|--------|
| > 1ms without `.await` → `spawn_blocking` | CPU work in async starves the executor; p99 impact is severe |
| Bounded channels only | `mpsc::unbounded_channel` → OOM under load — always set capacity |
| `bytes::Bytes` for HTTP payloads | ~30% allocation reduction vs. `Vec<u8>` |
| `tokio::sync::Mutex` in async context | `std::sync::Mutex` → deadlock risk under `.await` while locked |
| Remove debug logs from hot paths | 40% throughput gain in production case |
| No spawn storms | Batch with `JoinSet` + semaphore; don't spawn 100K tasks in a loop |

Real-world: moving a 5ms JSON serialization step into `spawn_blocking` dropped p99 from 450ms → 80ms.

---

## Canonical Middleware Stack Order

```rust
// Layers execute bottom-to-top on request, top-to-bottom on response
Router::new()
    .route(...)
    .layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(TraceLayer::new_for_http())
            .layer(TimeoutLayer::new(Duration::from_secs(30)))
            .layer(CompressionLayer::new())
            .layer(CorsLayer::permissive()),
    )
    .with_state(state)
```

---

## Tokio — Mutex Rules (2026 Clarification)

**Key finding**: The 2026 recommendation is the OPPOSITE of the previous guidance. `std::sync::Mutex` is the default; `tokio::sync::Mutex` is only for cases where the guard must be held across an `.await`.

| Mutex type | When to use |
|------------|-------------|
| `std::sync::Mutex` | Default — short-lived critical sections with no await while locked |
| `tokio::sync::Mutex` | Only when guard must be held across `.await` |

**Guard scoping pattern** — always scope guards explicitly:
```rust
let value = {
    let g = mutex.lock().unwrap();
    g.value.clone()
};  // guard dropped here
expensive_async_op(value).await;
```

**`Notify` + `std::Mutex` pattern** — preferred over `tokio::Mutex` for state-machine signaling:
```rust
let state = Arc::new(Mutex::new(State::default()));
let notify = Arc::new(Notify::new());
{ state.lock().unwrap().ready = true; }
notify.notify_one();
notify.notified().await;  // await is outside the lock
```

**Deadlock trap**: If the same `tokio::sync::Mutex` is locked twice in one task, the second `lock().await` parks forever. Always scope guards to avoid holding them longer than necessary.

Sources: [tokio shared state tutorial](https://tokio.rs/tokio/tutorial/shared-state), [e6data deadlock analysis](https://www.e6data.com/blog/deadlocking-tokio-mutex-without-holding-lock)

---

## sqlx 0.8 — Pool Tuning (Updated)

**`statement_cache_capacity`** — default is 100 (LRU per connection). Production recommendation: 512 for fixed queries; 0 behind PgBouncer in transaction-pooling mode.

```rust
PgPoolOptions::new()
    .statement_cache_capacity(512)   // up from default 100
    .test_before_acquire(false)      // skip ping — saves one round-trip per query
    // ... other options
```

**`test_before_acquire(false)`**: Default `true` adds a ping round-trip to every `pool.acquire()`. Safe to disable when `max_lifetime` + `idle_timeout` handle stale connections — saves measurable latency in production.

**`acquire_timeout` must be shorter than HTTP timeout**: 5s recommended so callers fail fast on pool exhaustion (not 30s default which silently queues behind a stuck request).

Sources: [sqlx statement caching (DeepWiki)](https://deepwiki.com/launchbadge/sqlx/4.5-statement-caching), [oneuptime connection pooling 2026](https://oneuptime.com/blog/post/2026-01-07-rust-database-connection-pooling/view)

---

## tower_http::TimeoutLayer vs tower::TimeoutLayer

`tower_http::timeout::TimeoutLayer` returns `408 Request Timeout` automatically. `tower::timeout::TimeoutLayer` requires a `HandleErrorLayer` wrapper.

```rust
// CORRECT — tower_http, auto 408
use tower_http::timeout::TimeoutLayer;
.layer(TimeoutLayer::new(Duration::from_secs(30)))

// OLD — tower, requires extra HandleErrorLayer
.layer(HandleErrorLayer::new(|_| async { StatusCode::REQUEST_TIMEOUT }))
.layer(tower::timeout::TimeoutLayer::new(...))
```

**Streaming vs non-streaming**: Apply `TimeoutLayer` only to non-streaming routes. SSE routes must be outside — `tower_http::TimeoutLayer` fires on total response duration.

---

## Bounded Channels — Load Shedding Pattern

`try_send` (non-blocking) at ingestion points is the 2026 idiomatic pattern for explicit backpressure:

```rust
match tx.try_send(work) {
    Ok(()) => {},
    Err(TrySendError::Full(_)) => metrics::increment("dropped"),
    Err(TrySendError::Closed(_)) => return,
}
```

Never use `mpsc::unbounded_channel` — shifts memory pressure to heap invisibly. Bounded channels make backpressure explicit and predictable.

---

## Sources

- [Rust Async Killed Your Throughput (Medium 2026)](https://medium.com/@shkmonty35/rust-async-just-killed-your-throughput-and-you-didnt-notice-c38dd119aae5)
- [Database Connection Pooling in Rust with SQLx (oneuptime 2026)](https://oneuptime.com/blog/post/2026-01-07-rust-database-connection-pooling/view)
- [Building High-Performance APIs with Axum (dasroot.net 2026)](https://dasroot.net/posts/2026/04/building-high-performance-apis-axum-rust/)
- [tokio discussions #7627 std vs tokio Mutex](https://github.com/tokio-rs/tokio/discussions/7627)
- [sqlx PgConnectOptions docs.rs](https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgConnectOptions.html)
- [Why Rust Microservices Using SQLx Are Doing Connection Pooling Wrong (Medium)](https://medium.com/@theopinionatedev/why-every-rust-microservice-using-sqlx-is-doing-connection-pooling-wrong-cf809b5601b3)
