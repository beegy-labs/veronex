# Code Patterns: Rust — Async, Concurrency & Performance

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## async-trait (Required)

`#[async_trait]` still required for `Arc<dyn Trait>`. Rust 1.75+ async fn in trait is object-safe with `impl Trait` only, not `dyn Trait`.

```rust
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
  async fn get_by_hash(&self, hash: &str) -> anyhow::Result<Option<ApiKey>>;
}
```

## Mutex Rules — `std` vs `tokio`

**Default: use `std::sync::Mutex`.** Only reach for `tokio::sync::Mutex` when the guard must be held across an `.await`.

```rust
// CORRECT — std Mutex for short-lived, sync-only critical sections
let value = {
    let g = std::sync::Mutex::lock(&state).unwrap();
    g.counter  // clone/copy before dropping
};  // guard dropped — no await while locked
expensive_async_op(value).await;

// CORRECT — tokio Mutex only when guard spans an await
let g = tokio_mutex.lock().await;
some_async_fn().await;  // guard still held — yields instead of blocking the thread
```

| Rule | Detail |
|------|--------|
| `std::sync::Mutex` (default) | No await-held risk; lower overhead |
| `tokio::sync::Mutex` | Only when you must hold the guard across `.await` |
| Never expose guard to async callers | Wrap in a struct; lock inside non-async methods only |
| Never acquire the same `tokio::sync::Mutex` twice in one task | Deadlock — second `lock().await` parks forever |

**Fetch/Apply split** — when a `tokio::Mutex` guards in-memory state that needs refreshing from an async source, separate the async fetch from the sync apply to minimize lock hold time:

```rust
// CORRECT — async I/O outside the lock, sync state update inside
async fn refresh(&self) {
    // 1. Lock-free async fetch — no lock held during network I/O
    let fresh = self.fetch_from_remote().await;

    // 2. Apply synchronously with lock held — no await inside
    let mut state = self.state.lock().await;
    if let Some(data) = fresh {
        Self::apply_update(&mut state, &data);
    }
}   // guard drops here

// WRONG — holds lock across HTTP await (blocks Tokio thread during I/O)
async fn refresh_bad(&self) {
    let mut state = self.state.lock().await;   // lock acquired
    let fresh = self.http_client.get(...).await?;  // await while locked
    state.data = fresh;
}
```

**`Notify` + `std::Mutex` pattern** — preferred over `tokio::Mutex` for state-machine signaling:
```rust
let state = Arc::new(Mutex::new(State::default()));
let notify = Arc::new(Notify::new());
{ state.lock().unwrap().ready = true; }  // sync lock, no await inside
notify.notify_one();
notify.notified().await;                 // await is outside the lock
```

## DashMap (not `Mutex<HashMap>`)

```rust
let jobs: Arc<DashMap<Uuid, JobEntry>> = Arc::new(DashMap::new());
jobs.insert(id, entry);
let value = jobs.get(&id).map(|r| r.clone());  // clone, drop Ref before .await
let notify = {
  let mut entry = jobs.get_mut(&id).ok_or(NotFound)?;
  entry.tokens.push(token);
  entry.notify.clone()
};  // RefMut dropped here
notify.notify_one();
```

Never hold `Ref`/`RefMut` across `.await` -- it locks the shard.

## Performance Patterns

> Full research: `research/backend/rust-perf-2026.md`

**Enum `as_str()`** -- zero-allocation. Never `format!("{:?}", e).to_lowercase()`.

```rust
impl FinishReason {
  pub fn as_str(&self) -> &'static str {
    match self { Self::Stop => "stop", Self::Length => "length", ... }
  }
}
```

**Streaming hash** -- `io::Write` adapter for digest, zero intermediate allocation:

```rust
struct HashWriter<D: Digest>(D);
impl<D: Digest> io::Write for HashWriter<D> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> { self.0.update(buf); Ok(buf.len()) }
  fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
// serde_json::to_writer(&mut w, &value)
```

**`Vec::reserve()`** before extend: `accumulated.reserve(arr.len())` then `extend`.

## VramPool CAS Safety

`try_reserve()` uses compare-and-swap with `MAX_CAS_RETRIES = 16`:

```rust
for _ in 0..MAX_CAS_RETRIES {
    let current = active_kv.load(Ordering::Acquire);
    if current + kv > kv_budget { return None; }
    if active_kv.compare_exchange_weak(current, current + kv, ...).is_ok() {
        return Some(permit);
    }
}
```

## tokio — LTS Pin + Manual Runtime Builder

Pin to **tokio 1.47** (LTS until 2026-09; next LTS 1.52 EOL 2027-09). Never use `#[tokio::main]` in production binaries — tune the runtime explicitly:

```toml
# Cargo.toml
tokio = { version = "~1.47", features = ["rt-multi-thread", "macros", "signal", "sync", "time"] }
```

```rust
// main.rs
fn main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(num_cpus::get())
        .max_blocking_threads(512)             // blocks: sqlx fallback, tracing-appender, DNS
        .thread_name("veronex-worker")
        .enable_all()
        .build()?;
    rt.block_on(async_main())
}
```

Rules:
- Always provide `thread_name` — observability tools group by thread name.
- `max_blocking_threads` sized for the sum of pool backlogs (DB + blocking FS + DNS).
- Never call `.block_on()` from an async context — use `tokio::task::block_in_place` only at adapter edges.

## Background Tasks -- JoinSet + CancellationToken

```rust
let shutdown = CancellationToken::new();
let mut tasks = JoinSet::new();
tasks.spawn(run_health_checker_loop(..., shutdown.child_token()));
axum::serve(listener, app)
  .with_graceful_shutdown(shutdown.clone().cancelled_owned()).await?;
shutdown.cancel();
while let Some(res) = tasks.join_next().await {
  if let Err(e) = res { tracing::warn!("task panicked: {e}"); }
}
```

Loop convention: accept `CancellationToken`, use `select!` to exit cleanly.

### Stats Ticker — Sliding Window Counters

`FlowStats` uses 60 x 1-second sliding-window buckets (not ring-buffer event scanning):

| Field | Computation | Buckets |
|-------|-------------|---------|
| `incoming` | sum of last 10 buckets | req/s = incoming/10 |
| `incoming_60s` | sum of all 60 buckets | = req/m |
| `completed` | sum of all 60 buckets | terminal events |

A separate task counts broadcast events (`pending` -> incoming, terminal -> completed) into the current bucket. The ticker rotates buckets every second, clears the new slot, and always broadcasts -- no PartialEq skip. Clients rely on receiving stats every second.

`queued`/`running` sourced from DashMap (`get_live_counts()`) with DB fallback (single indexed query) when DashMap is empty (e.g. after restart). Not Valkey LLEN -- pops too fast for accurate reads.

## Lifecycle Port Pattern (Phase 1 ↔ Phase 2 SoD)

Outbound ports for long-running side effects (model load that may exceed 160s
on 200K-context models) split into two traits and combine via super-trait:

```rust
#[async_trait]
pub trait InferenceProviderPort: Send + Sync {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult>;
    fn stream_tokens(&self, job: &InferenceJob)
        -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>;
}

#[async_trait]
pub trait ModelLifecyclePort: Send + Sync {
    async fn ensure_ready(&self, model: &str)
        -> Result<LifecycleOutcome, LifecycleError>;
    async fn instance_state(&self, model: &str) -> ModelInstanceState;
    async fn evict(&self, model: &str, reason: EvictionReason)
        -> Result<(), LifecycleError>;
}

pub trait LlmProviderPort: InferenceProviderPort + ModelLifecyclePort {}
impl<T> LlmProviderPort for T
    where T: InferenceProviderPort + ModelLifecyclePort + ?Sized {}
```

Rules:
- One adapter implements **both** ports — concrete type satisfies the super-trait
  via the blanket impl. No double `Arc`.
- Composition root holds `Arc<dyn LlmProviderPort>` so call sites dispatch
  either super-trait method without owning two trait objects.
- Cloud / no-VRAM adapters provide a no-op `ModelLifecyclePort` returning
  `LifecycleOutcome::AlreadyLoaded` immediately.
- Concurrent `ensure_ready(M)` per `(provider, model)` coalesces on a
  `DashMap<String, Arc<LoadInFlight>>` slot:
  - leader runs the probe + `tokio::select!` over (probe future, stall detector,
    hard cap)
  - followers `Notify::notified().await` then read a `OnceCell` for the result
  - returns `LoadCompleted{duration_ms}` (leader) or `LoadCoalesced{waited_ms}` (follower)
- Errors typed as `LifecycleError` (LoadTimeout / Stalled / ProviderError /
  CircuitOpen / ResourcesExhausted) — the runner branches on cause without
  string-matching `anyhow::Error`.

SDD: `.specs/veronex/inference-lifecycle-sod.md`. Flow: `flows/model-lifecycle.md`.
