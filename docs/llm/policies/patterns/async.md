# Code Patterns: Rust — Async, Concurrency & Performance

> SSOT | **Last Updated**: 2026-05-02 | Classification: Operational
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

## Fan-out per-N awaits

`for x in collection { x.await }` produces O(N) wall-clock round-trips — biggest source of dispatcher latency at 10k-provider / 1M-TPS scale. Every loop with independent iterations must fan out.

| Pattern | Use when | Replaces |
|---------|----------|----------|
| `futures::future::join_all` | homogeneous result collection | `for x in xs { results.push(f(x).await) }` |
| `tokio::join!` | fixed pair of independent awaits | `a.await; b.await;` |
| `pool.mget(keys)` (Valkey MGET) | every iteration is `pool.get(key)` | `for k in ks { pool.get(k).await }` |
| `JoinSet` | heterogeneous results / cancellation needed | detached `tokio::spawn` accumulation |

Permitted sequential `.await` (intentional, audit-allowed): ordered reaper logic, lazy migration, `notify.notified()`, bounded image batches (≤4), background loops already running under `JoinSet`.

Canonical examples: `provider_router::filter_by_model_selection` (join_all), `runner::run_job` decr_pending+incr_running (tokio::join!), `analyzer.rs` demand counters / `inference_helpers::lookup_model_max_ctx` (MGET).

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

For long-running side effects (model load > 160s on 200K-context), split outbound capabilities into `InferenceProviderPort` (infer / stream) and `ModelLifecyclePort` (ensure_ready / instance_state / evict), then combine via super-trait `LlmProviderPort` with a blanket impl. One concrete adapter implements both; cloud / no-VRAM adapters provide a no-op `ModelLifecyclePort` returning `LifecycleOutcome::AlreadyLoaded` immediately. Composition root holds `Arc<dyn LlmProviderPort>`.

Concurrent `ensure_ready(model)` per `(provider, model)` coalesces on a `DashMap<String, Arc<LoadInFlight>>` slot: leader runs the probe under `tokio::select!` (probe future + `/api/ps` poller + stall detector + observability + hard cap); followers `Notify::notified().await` then read the `OnceCell` result. Errors typed as `LifecycleError` (LoadTimeout / Stalled / ProviderError / CircuitOpen / ResourcesExhausted) so the runner branches without string-matching.

**Sentinel-zero stall** (single-shot probe with no streamed progress): `last_progress_at: Arc<AtomicU64>` initialised to `0` is the "no signal yet" sentinel. Stall arm in the leader's `select!` skips while the value is `0`; only a separate observer (e.g. `/api/ps` poller) writes the timestamp. The probe HTTP is **never cancelled** by stall/hard-cap winners — closing the connection aborts the upstream load (ollama#8006). Use `MissedTickBehavior::Delay` on every interval-driven arm to prevent cascading bursts.

SDD: `.specs/veronex/history/inference-lifecycle-sod.md`. Flow: `flows/model-lifecycle.md`.
