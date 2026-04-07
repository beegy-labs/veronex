# Rust / Axum — 2026 Updates

> **Last Researched**: 2026-04-07 | **Source**: Axum changelog + web search
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

## Sources

- [Rust Async Killed Your Throughput (Medium 2026)](https://medium.com/@shkmonty35/rust-async-just-killed-your-throughput-and-you-didnt-notice-c38dd119aae5)
- [Database Connection Pooling in Rust with SQLx (oneuptime 2026)](https://oneuptime.com/blog/post/2026-01-07-rust-database-connection-pooling/view)
- [Building High-Performance APIs with Axum (dasroot.net 2026)](https://dasroot.net/posts/2026/04/building-high-performance-apis-axum-rust/)
