# Code Patterns: Rust — Tower Middleware & Routing

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## Tower Layer Order — `ServiceBuilder` SSOT

All routers compose layers via `tower::ServiceBuilder` in this exact order. Top executes first on the request path.

```rust
use tower::ServiceBuilder;
use tower_http::{
    trace::TraceLayer,
    catch_panic::CatchPanicLayer,
    timeout::TimeoutLayer,
    cors::CorsLayer,
    compression::CompressionLayer,
    decompression::RequestDecompressionLayer,
    sensitive_headers::SetSensitiveRequestHeadersLayer,
};

Router::new()
    .route("/v1/providers", post(create_provider))
    .layer(
        ServiceBuilder::new()
            .layer(SetSensitiveRequestHeadersLayer::from_shared(SENSITIVE.into())) // 1. redact before logging
            .layer(TraceLayer::new_for_http())                                     // 2. start span after redaction
            .layer(CatchPanicLayer::new())                                         // 3. convert panics to 500
            .layer(TimeoutLayer::new(JWT_ROUTER_TIMEOUT))                          // 4. cap total duration
            .layer(CorsLayer::permissive())                                        // 5. preflight first-class
            .layer(RequestDecompressionLayer::new())                               // 6. decompress request body
            .layer(CompressionLayer::new())                                        // 7. compress response body
            // 8+. auth / rate-limit / per-key concurrency
    );
```

Invariants:
- `SetSensitiveRequestHeadersLayer` MUST precede `TraceLayer` — logs otherwise leak Authorization.
- `TimeoutLayer` MUST precede auth — unauthenticated slow senders should not hold handler slots.
- Never use `axum::middleware::from_fn` when a tower layer exists — `from_fn` clones the handler's `AppState` per request.
- Streaming routes (SSE, chunked) live in a separate `Router` without `TimeoutLayer` (see next section).

## TimeoutLayer — `tower_http` over `tower`

Use `tower_http::timeout::TimeoutLayer` (returns `408 Request Timeout` automatically) instead of `tower::timeout::TimeoutLayer` (requires a `HandleErrorLayer` wrapper).

```rust
// CORRECT — tower_http auto-returns 408, no extra wiring
use tower_http::timeout::TimeoutLayer;
.layer(TimeoutLayer::new(JWT_ROUTER_TIMEOUT))

// WRONG — requires manual HandleErrorLayer to map error → HTTP response
use tower::timeout::TimeoutLayer;
.layer(HandleErrorLayer::new(|_| async { StatusCode::REQUEST_TIMEOUT }))
.layer(tower::timeout::TimeoutLayer::new(JWT_ROUTER_TIMEOUT))
```

**Streaming vs. non-streaming:** Apply `TimeoutLayer` only to non-streaming routes. SSE/chunked routes must be outside the timeout layer (or use a much larger value), as `tower_http::TimeoutLayer` fires on total response duration.

```rust
// CORRECT — SSE route merged into a separate Router without TimeoutLayer
let jwt_router = Router::new()
    .route("/v1/something", get(handler))
    .route_layer(/* auth */)
    .layer(TimeoutLayer::new(JWT_ROUTER_TIMEOUT));  // fires on total duration

// Merge SSE route WITHOUT the timeout layer
let app = Router::new()
    .merge(jwt_router)
    .merge(
        Router::new()
            .route("/v1/dashboard/jobs/stream", get(job_events_sse))
            .route_layer(middleware::from_fn_with_state(state.clone(), jwt_auth)),
        // No TimeoutLayer — stream runs until client disconnects
    );

// WRONG — SSE inside the same .layer(TimeoutLayer) block
// → stream will be killed after JWT_ROUTER_TIMEOUT (e.g. 30s)
```

## Per-Key Concurrent Connection Limit (LLM Gateway)

Protects against Slowloris-style abuse and noisy-neighbor tenant exhaustion (OWASP API4:2023 / LLM10:2025). RPM alone cannot defend against slow-sender attacks.

```rust
// Middleware state — per-key semaphore via DashMap
#[derive(Clone)]
pub struct PerKeyConcurrency {
    semaphores: Arc<DashMap<String, Arc<Semaphore>>>,
    max: usize,
}
impl PerKeyConcurrency {
    pub fn new(max: usize) -> Self { Self { semaphores: Arc::new(DashMap::new()), max } }
    fn semaphore(&self, key: &str) -> Arc<Semaphore> {
        self.semaphores.entry(key.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(self.max)))
            .clone()
    }
}

// In middleware: try_acquire (hard 429, no queue) — never queue under LLM load
let sem = state.semaphore(&api_key);
let _permit = sem.try_acquire_owned()
    .map_err(|_| AppError::TooManyRequests { retry_after: 1 })?;
// permit drops when response completes — slot released automatically
```

| Tier | Max concurrent | Rationale |
|------|---------------|-----------|
| Standard / free | 4 | Prevents budget exhaustion on expensive models |
| Paid / team | 8 | Matches provider soft limits |
| Internal | 16 | Full throughput, monitored |

Rule: use `try_acquire` (immediate 429), not `acquire` (queue). Queued permits under flood hold tasks indefinitely.

**Anti-pattern — AtomicU32 + manual RAII guard:**
```rust
// WRONG — race window between load and store; RAII struct needed; harder to reason about
struct InFlightGuard { counter: Arc<AtomicU32> }
impl Drop for InFlightGuard { fn drop(&mut self) { self.counter.fetch_sub(1, Ordering::Relaxed); } }
let current = counter.fetch_add(1, Ordering::Relaxed);
if current >= MAX { counter.fetch_sub(1, Ordering::Relaxed); return 429; }
let _guard = InFlightGuard { counter: counter.clone() };
```
Use `Semaphore::try_acquire_owned()` — the permit is the guard; no custom RAII needed.

## Adding a New Port + Adapter

| Step | File | Action |
|------|------|--------|
| 1 | `domain/entities/new_entity.rs` | Pure struct, no I/O |
| 2 | `application/ports/outbound/new_port.rs` | `#[async_trait]` trait; add to mod.rs |
| 3 | `docker/postgres/init.sql` | Add column/table to consolidated schema |
| 4 | `infrastructure/outbound/persistence/new.rs` | Impl trait; add to mod.rs |
| 5 | `infrastructure/inbound/http/state.rs` | `Arc<dyn NewPort>` field |
| 6 | `main.rs` | Init + inject into AppState |
| 7 | `infrastructure/inbound/http/new_handlers.rs` | `Result<T, AppError>` |
| 8 | `infrastructure/inbound/http/router.rs` | Register routes inside auth middleware |
| 9 | `docs/llm/{domain}/new_feature.md` | CDD doc |

## Cross-Module Error Sentinel Constants

Use a `const &str` to share error markers across module boundaries instead of duplicating string literals.

```rust
// session.rs — define once
pub(crate) const SESSION_EXPIRED_MARKER: &str = "session expired";

// client.rs — use in error construction
return Err(anyhow!("MCP {SESSION_EXPIRED_MARKER} (404) for {}", session.url));

// bridge.rs — use in match guard
Err(e) if e.to_string().contains(SESSION_EXPIRED_MARKER) => { ... }
```

Prevents silent drift when one side is renamed. `pub(crate)` keeps the sentinel internal.

