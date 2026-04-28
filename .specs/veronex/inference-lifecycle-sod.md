# SDD: Inference Lifecycle SoD (model-load ↔ inference)

> Status: planned | Change type: **Add** (new outbound port + new use-case phase) | Created: 2026-04-28 | Owner: TBD
> CDD basis: `docs/llm/policies/architecture.md` (hexagonal + multi-provider routing) · `docs/llm/inference/capacity.md` (VramPool SSOT) · `docs/llm/inference/mcp.md` (bridge run_loop)
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S14
> **Resume rule**: every section is self-contained. Any future session reading this SDD alone (no chat history) must be able to continue from the last unchecked box.

---

## §0 Quick-resume State

Mark with `[x]` when committed. Each Tier has its own §-block with file paths, exact changes, acceptance tests.

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| A — Domain + Port + Mock | [x] done | `feat/lifecycle-port-mock` | #91 | `7a5d8f2` |
| B — OllamaAdapter lifecycle | [x] done | `feat/ollama-lifecycle` | #92 | `2bce27b` |
| C — Runner integration + flag | [x] done | `feat/runner-lifecycle-phase` | #93 | `4b816a4` |
| CDD-sync (post C) | [x] done | `docs/cdd-lifecycle` | TBD | TBD |
| Flow-sync (post C) | [x] done | `docs/cdd-lifecycle` (same branch) | TBD | TBD |
| Live verify (dev) | [ ] pending | — | flag flip on dev cluster | — |

If you find this SDD with all boxes unchecked, start at §A. If A is checked, start at §B. Etc.

---

## §1 Problem (verified)

`InferenceProviderPort::stream_tokens` implicitly performs both Phase 1 (model load: weight + KV cache + warmup) and Phase 2 (token generation). Single conflated timeout produces three concrete defects:

| # | Defect | Evidence (live measurement, 2026-04-28) |
|---|--------|---------------------------------------|
| D1 | First request after idle to a 200K-context model exceeds bridge timeout silently — empty content returned | `qwen3-coder-next-200k:latest` direct ollama probe: `load_duration` = **163,671 ms**. ollama log: `client connection closed before server finished loading, aborting load`. veronex bridge had `COLLECT_ROUND_TIMEOUT=45s` (now 240s after #90). |
| D2 | Operator cannot distinguish "load slow" from "inference slow" | Single tracing span; no per-phase split metric |
| D3 | Concurrent same-model requests trigger N parallel load probes | `OllamaAdapter::stream_tokens` has no in-flight dedup before HTTP submit |

veronex#90 (commit `22887f1` on `develop`) introduced phased timeouts inside `collect_round` (FIRST_TOKEN=240s / STREAM_IDLE=45s / ROUND_TOTAL=360s) — **defense-in-depth retained**, but does not solve D2/D3 and only widens D1's window.

---

## §2 Root Cause

`InferenceProviderPort` contract elides the load-state precondition. Adapters silently load on first stream request. There is no first-class lifecycle abstraction in the application layer. Architecture vision (`docs/llm/policies/architecture.md` §Vision) designates VramPool as model-lifecycle owner — VramPool is bookkeeping (which model is on which provider) but does not own the load execution path. The execution gap means timing/failure policy gets nailed down at the wrong layer (single HTTP timeout), violating hexagonal SoD.

---

## §3 Solution (vision-aligned)

### §3.1 Architecture target

```
domain/value_objects/model_instance_state.rs      [NEW]
  enum ModelInstanceState { NotLoaded | Loading | Loaded | Failed | Evicted }
  enum EvictionReason     { VramPressure | KeepAliveExpired | Operator | LoadFailed }

domain/errors.rs                                  [+LifecycleError variants]

application/ports/outbound/
  model_lifecycle.rs                              [NEW]
    trait ModelLifecyclePort {
      ensure_ready(&self, model)            -> Result<LifecycleOutcome, LifecycleError>
      instance_state(&self, model)           -> ModelInstanceState
      evict(&self, model, reason)            -> Result<(), LifecycleError>
    }
    enum LifecycleOutcome { AlreadyLoaded | LoadCompleted{duration_ms} | LoadCoalesced{waited_ms} }

  inference_provider.rs                            [REFINED CONTRACT]
    // doc: PRECONDITION — caller must invoke ModelLifecyclePort::ensure_ready first

application/use_cases/inference/runner.rs          [MODIFIED]
  process_job:
    provider = dispatcher.select(job)              ← unchanged (queue, thermal, CB, VRAM, locality)
    provider.ensure_ready(model)?              ← Phase 1 (NEW)
      ├─ thermal_throttle_map.guard(provider)?     ← reuse existing
      ├─ provider_circuit_breaker.guard()?         ← reuse existing (single CB per provider)
      ├─ vram_pool.is_loaded(provider, model)? → AlreadyLoaded
      └─ in-flight dedup + probe load
    state.vram_pool.record_loaded(provider, model, outcome)
    provider.stream_tokens(job)                ← Phase 2 (warm guaranteed)

infrastructure/outbound/ollama/
  lifecycle.rs                                  [NEW]
    LoadInFlight { started_at, notify, last_progress_at, result: OnceCell }
    ProbeRunner   { POST /api/generate {prompt:"", num_predict:0, keep_alive:"30m"} }
    // NO PsCache (VramPool is SSOT — see §3.2)
    // NO LifecycleCircuitBreaker (reuse per-provider CB — see §3.3)

  adapter.rs                                    [+ModelLifecyclePort impl]
    in_flight_loads: DashMap<String, Arc<LoadInFlight>>     ← per-(this provider, model) coalescing

infrastructure/outbound/gemini/adapter.rs        [+no-op ModelLifecyclePort]
  ensure_ready returns AlreadyLoaded immediately (cloud — no local lifecycle)

infrastructure/outbound/capacity/vram_pool.rs    [+record_loaded / record_evicted / is_loaded]
  Sole SSOT for "is model X loaded on provider P". Already polled by sync_loop (30s).
```

### §3.2 SSOT integrity — VramPool is single source

Veronex vision: VramPool manages model lifecycle accounting. We do NOT add a parallel `PsCache`. Instead:

- `ensure_ready` queries `vram_pool.is_loaded(provider, model)` first (in-process, O(1) DashMap lookup).
- `vram_pool` synced by existing `sync_loop` (30s) which already polls `/api/ps`.
- `ensure_ready` writes to `vram_pool` after probe success (`record_loaded`).
- `ensure_ready` does NOT poll `/api/ps` itself — relies on sync_loop's freshness OR triggers probe-load (which itself confirms load via response).

→ **No new cache, no SSOT divergence.**

### §3.3 Circuit breaker — single per-provider CB

Veronex vision: "circuit breaker per provider" (single per provider). Earlier draft proposed a separate `LifecycleCircuitBreaker` — that **violates** the vision. Corrected design:

- Reuse existing `provider_circuit_breaker` (single per provider).
- Add a `failure_reason` enum tag when recording failures: `LoadFailed | InferenceFailed | StreamError`.
- CB trip threshold counts ALL failures regardless of phase.
- Observability differentiates by `failure_reason` label, not by separate CB instances.

```rust
// infrastructure/outbound/ollama/circuit_breaker.rs (existing, +tag)
pub enum FailureReason { LoadFailed, InferenceFailed, StreamError, ProviderUnreachable }
impl ProviderCircuitBreaker {
    pub fn record_failure(&self, reason: FailureReason) { ... }
}
```

### §3.4 Thermal + queue compliance

Veronex vision: "Enqueue before GPU work" + "thermal protection per provider".

`ensure_ready` triggers VRAM allocation = GPU work. Therefore:

- Called **after** `dispatcher.select` (which already filters thermal-throttled providers + CB-open providers + VRAM-insufficient providers).
- Lives in `runner.rs` (post-dispatch), not in `bridge.rs` (pre-queue).
- Inside `ensure_ready`, also re-check `provider_circuit_breaker.guard()?` and `thermal_map.is_throttled(provider)?` (defensive — race window between dispatch and runner is bounded but non-zero).

→ Queue + thermal compliance preserved.

### §3.5 Multi-model / multi-provider behavior matrix

| Scenario | Path |
| -------- | ---- |
| Provider P has model A loaded; request A → dispatcher picks P (locality bonus) | `ensure_ready(A)` → VramPool says loaded → `AlreadyLoaded` instant |
| Model A loaded on P1+P2; dispatcher picks P1 (highest score) | P1's adapter `ensure_ready(A)` cache-hit |
| Cold model B request → dispatcher picks P (with VRAM) | `ensure_ready(B)` triggers probe; concurrent same-model requests on same adapter coalesce on `LoadInFlight.notify` |
| ollama evicts A to load B (ollama-side TTL) | sync_loop picks up new state next cycle; `ensure_ready(A)` next request → cold load |
| Provider P circuit-open | `dispatcher.select` skips P; if races slip through, `ensure_ready` rejects via CB.guard with `CircuitOpen` |
| MCP loop with 5 rounds | each round invokes `ensure_ready` on selected provider. dispatcher locality keeps same provider; AlreadyLoaded after round 1 |
| Multiple veronex pods, same provider, same model, concurrent cold | each pod has its own in-flight dedup. ollama itself is idempotent on duplicate /api/generate probes (loads once). cross-pod coordination is **out of scope** (ollama's idempotency is sufficient at our scale) |

### §3.6 Defense-in-depth layers

```
┌──────────────────────────────────────────────────────────────────┐
│ HTTP route layer        INFERENCE_ROUTER_TIMEOUT 360s            │
│  └─ Bridge run_loop      ROUND_TOTAL_TIMEOUT 360s (defense)     │
│      └─ Runner.process_job                                       │
│          ├─ Dispatcher gate: queue + thermal + CB + VRAM         │
│          ├─ Phase 1 lifecycle    LIFECYCLE_LOAD_TIMEOUT 600s     │
│          │     └─ Stall detect   STALL 60s                       │
│          │     └─ Per-provider CB (reused, with reason tag)      │
│          └─ Phase 2 inference                                    │
│              └─ Bridge collect_round (PR #90 phased)             │
│                  ├─ FIRST_TOKEN_TIMEOUT 240s (defense)           │
│                  │   (post-Tier-C: tighten to 30s — warm guaranteed) │
│                  ├─ STREAM_IDLE_TIMEOUT 45s                      │
│                  └─ Provider HTTP REQUEST_TIMEOUT 300s           │
└──────────────────────────────────────────────────────────────────┘
```

PR #90 phased timeout retained. Tier C optionally tightens FIRST_TOKEN_TIMEOUT after live verification.

---

## §4 Vision Alignment Check (after redesign)

| Vision (`docs/llm/policies/architecture.md` §Vision) | This SDD |
| ---------------------------------------------------- | -------- |
| 1. Autonomous intelligence (학습/예측/결정) | ⚠️ Partial — reactive only. Predictive lifecycle is a follow-up SDD (`inference-lifecycle-autonomous.md`) |
| 2. Single compute pool | ⚠️ Partial — single-pod dedup; cross-pod coordination via ollama idempotency (sufficient at current scale) |
| 3. Maximize utilization, minimize waste | ✓ — coalescing prevents duplicate loads; VramPool tracks utilization |
| 4. Power efficiency | ✓ — `ensure_ready` only loads when VramPool says NotLoaded. existing `OLLAMA_KEEP_ALIVE=10m` (project memory: low_power_ollama_lifecycle) preserved. predictive preload deferred |
| 5. Multi-model co-residence + locality | ✓ — dispatcher locality bonus reused |
| 6. 3-phase adaptive learning | ✓ — `record_loaded` triggers Cold Start AIMD epoch reset (per `capacity.md`) |
| 7. Self-healing | ✓ — per-provider CB single instance with `failure_reason` tag |
| 8. Thermal protection | ✓ — `dispatcher.select` filters; `ensure_ready` re-checks defensively |
| Scale 10K / O(1) hot path | ✓ — VramPool DashMap O(1); /api/ps polled only by sync_loop, not per-request |
| ZSET queue / no GPU bypass | ✓ — `ensure_ready` invoked post-dispatch (after queue) |
| VramPool SSOT | ✓ — single source, no parallel cache |

→ **~85% aligned**. Predictive intelligence (Vision #1) defers to follow-up SDD.

---

## §5 Tier A — Domain + Port + Mock

> Goal: deliver port + domain types + in-memory mock. Zero production behavior change.
> Estimate: ~250 LoC + tests. Branch: `feat/lifecycle-port-mock`.

### §5.1 Files to create / modify

| File | Action | Purpose |
| ---- | ------ | ------- |
| `crates/veronex/src/domain/value_objects.rs` | MODIFY (append section) | enum `ModelInstanceState` + `EvictionReason` (extend existing single-file pattern) |
| `crates/veronex/src/domain/errors.rs` | MODIFY | add `LifecycleError` variants |
| `crates/veronex/src/application/ports/outbound/model_lifecycle.rs` | CREATE | trait `ModelLifecyclePort` + `LifecycleOutcome` |
| `crates/veronex/src/application/ports/outbound/mod.rs` | MODIFY | re-export |
| `crates/veronex/src/application/ports/outbound/inference_provider.rs` | MODIFY | add precondition doc comment (no signature change) |

### §5.1a Domain types — exact Rust signatures

**`crates/veronex/src/domain/value_objects/model_instance_state.rs`** — full content:

```rust
//! Per-(provider, model) lifecycle state. Tracked in VramPool (SSOT).
//! Transition rules enforced via `try_transition`; invalid moves return Err.

use std::time::SystemTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelInstanceState {
    NotLoaded,
    Loading {
        started_at: SystemTime,
        last_progress_at: SystemTime,
    },
    Loaded {
        loaded_at: SystemTime,
        weight_bytes: u64,
    },
    Failed {
        failed_at: SystemTime,
        reason: String,
        retry_after: SystemTime,
    },
    Evicted {
        evicted_at: SystemTime,
        reason: EvictionReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionReason {
    VramPressure,        // ollama unloaded to make room for another model
    KeepAliveExpired,    // ollama TTL expired (low-power policy)
    Operator,            // explicit evict() call
    LoadFailed,          // load attempt failed; cleanup state
}

impl ModelInstanceState {
    /// Returns true if `self` may legally transition to `next`.
    /// Invariants:
    ///   NotLoaded → Loading | Failed
    ///   Loading   → Loaded | Failed
    ///   Loaded    → Evicted (must go via Evicted, not directly to NotLoaded)
    ///   Failed    → Loading (retry after retry_after)
    ///   Evicted   → NotLoaded | Loading
    pub fn can_transition_to(&self, next: &Self) -> bool {
        use ModelInstanceState::*;
        matches!(
            (self, next),
            (NotLoaded, Loading { .. }) | (NotLoaded, Failed { .. })
            | (Loading { .. }, Loaded { .. }) | (Loading { .. }, Failed { .. })
            | (Loaded { .. }, Evicted { .. })
            | (Failed { .. }, Loading { .. })
            | (Evicted { .. }, NotLoaded) | (Evicted { .. }, Loading { .. })
        )
    }
}
```

**`crates/veronex/src/domain/errors.rs`** — append `LifecycleError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("model load timed out after {elapsed_ms}ms (max {max_ms}ms)")]
    LoadTimeout { elapsed_ms: u64, max_ms: u64 },
    #[error("model load stalled — no progress for {last_progress_ms}ms")]
    Stalled { last_progress_ms: u64 },
    #[error("provider error during lifecycle: {0}")]
    ProviderError(String),
    #[error("provider circuit breaker open")]
    CircuitOpen,
    #[error("VRAM exhausted: available {available_vram_mb}MB, required {required_mb}MB")]
    ResourcesExhausted { available_vram_mb: u64, required_mb: u64 },
}
```

### §5.1b `ModelLifecyclePort` trait — exact signature

**`crates/veronex/src/application/ports/outbound/model_lifecycle.rs`** — full content:

```rust
//! Outbound port: model lifecycle (load/unload/health) — separates Phase 1
//! (resource acquisition) from Phase 2 (token generation, see InferenceProviderPort).
//!
//! Caller contract: invoke `ensure_ready` before `InferenceProviderPort::stream_tokens`.
//! Idempotent + coalesces concurrent same-model calls per (provider, model) pair.

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::errors::LifecycleError;
use crate::domain::value_objects::model_instance_state::{EvictionReason, ModelInstanceState};

#[async_trait]
pub trait ModelLifecyclePort: Send + Sync {
    /// Postcondition: returns Ok ⇒ model is in `Loaded` state on this provider.
    /// Caller may proceed to `stream_tokens` immediately on Ok.
    async fn ensure_ready(&self, model: &str) -> Result<LifecycleOutcome, LifecycleError>;

    /// Read-only snapshot — used by VramPool / capacity planner / dashboards.
    /// Does NOT trigger load.
    async fn instance_state(&self, model: &str) -> ModelInstanceState;

    /// Operator-driven eviction (e.g. model unenrolled, VRAM pressure rebalance).
    async fn evict(&self, model: &str, reason: EvictionReason) -> Result<(), LifecycleError>;
}

#[derive(Debug, Clone)]
pub enum LifecycleOutcome {
    /// `/api/ps` cache (via VramPool SSOT) said the model was already loaded.
    /// Returns in <1ms (DashMap lookup).
    AlreadyLoaded,
    /// We triggered the load; ollama returned 200 OK after `duration_ms`.
    LoadCompleted { duration_ms: u64 },
    /// Another in-flight load completed for us (coalesced). We waited `waited_ms`.
    LoadCoalesced { waited_ms: u64 },
}
```

### §5.1c `InferenceProviderPort` precondition doc

**`crates/veronex/src/application/ports/outbound/inference_provider.rs`** — modify the trait doc:

```rust
/// Outbound port for a single LLM inference provider (Ollama, Gemini, …).
///
/// **Precondition for `stream_tokens` and `infer`**: caller must invoke
/// `ModelLifecyclePort::ensure_ready` first. Adapters MAY perform a
/// lightweight VramPool check and return `InferenceError::ModelNotReady`
/// if the precondition is violated. Implicit auto-load behavior present
/// in earlier revisions (single 300s timeout) is deprecated.
#[async_trait]
pub trait InferenceProviderPort: Send + Sync {
    // ... existing methods unchanged ...
}
```

### §5.2 Acceptance criteria

- [ ] `cargo check -p veronex` clean
- [ ] `cargo test -p veronex --lib domain::value_objects::model_instance_state::` passes
- [ ] No production behavior change (no adapter touches yet)
- [ ] Doc comments cite this SDD path: `.specs/veronex/inference-lifecycle-sod.md`

### §5.3 Tests required (Tier A)

| Test | Assertion |
| ---- | --------- |
| `model_instance_state_serde_roundtrip` | (de)serialize each variant via JSON |
| `model_instance_state_transitions_valid` | invariants — NotLoaded→Loading allowed, Loaded→NotLoaded forbidden (must go via Evicted) |
| `lifecycle_error_display_includes_context` | each error variant's Display contains the actionable hint |
| `lifecycle_outcome_duration_recorded` | `LoadCompleted{duration_ms}` carries non-zero duration |

### §5.4 Resume note

If you find this section unchecked but Tier A files exist on a branch: `git log feat/lifecycle-port-mock --oneline` to see what's done. Re-run §5.2 acceptance to verify state. If acceptance passes, mark §0 box and move to §6.

---

## §6 Tier B — OllamaAdapter lifecycle implementation

> Goal: `OllamaAdapter` implements `ModelLifecyclePort`. Reuses `preloader.rs` probe pattern. Coalesces concurrent same-model loads.
> Estimate: ~600 LoC + tests. Branch: `feat/ollama-lifecycle`. Blocked on §5.

### §6.1 Files to create / modify

| File | Action | Purpose |
| ---- | ------ | ------- |
| `crates/veronex/src/infrastructure/outbound/ollama/lifecycle.rs` | CREATE | `LoadInFlight`, `ProbeRunner`, helpers |
| `crates/veronex/src/infrastructure/outbound/ollama/adapter.rs` | MODIFY | `impl ModelLifecyclePort for OllamaAdapter` (add `in_flight_loads: DashMap` field) |
| `crates/veronex/src/infrastructure/outbound/ollama/preloader.rs` | MODIFY | extract zero-token-generate probe to public `pub(super) async fn probe_load(client, base_url, model, keep_alive)`; existing startup callers pass `keep_alive: -1` |
| `crates/veronex/src/infrastructure/outbound/ollama/mod.rs` | MODIFY | re-export `lifecycle` |
| `crates/veronex/src/infrastructure/outbound/ollama/circuit_breaker.rs` | MODIFY | add `FailureReason` enum tag to `record_failure` (backward-compat default = `InferenceFailed`) |
| `crates/veronex/src/infrastructure/outbound/gemini/adapter.rs` | MODIFY | no-op `impl ModelLifecyclePort` (always `AlreadyLoaded`) |
| `crates/veronex/src/infrastructure/outbound/capacity/vram_pool.rs` | MODIFY | add `is_loaded(provider, model) -> bool`, `record_loaded(provider, model, outcome)`, `record_evicted(provider, model, reason)` |

### §6.2 Behavior contract for `OllamaAdapter::ensure_ready`

```
ensure_ready(model):
  // 1. Defensive provider-CB + thermal re-check (race window after dispatch)
  if circuit_breaker.is_open() return Err(CircuitOpen)
  if thermal_map.is_throttled(provider_id) return Err(ProviderError("thermal throttle"))

  // 2. SSOT lookup
  if vram_pool.is_loaded(provider_id, model) return Ok(AlreadyLoaded)

  // 3. In-flight coalesce
  if in_flight_loads.contains(model):
    wait on existing.notify
    return existing.result.cloned() with LoadCoalesced{waited_ms}

  // 4. Acquire slot, run probe, notify waiters
  slot = LoadInFlight::new()
  in_flight_loads.insert(model, slot)
  result = probe_with_stall_detection(model, keep_alive: "30m")
  slot.result.set(result.clone())
  slot.notify.notify_waiters()
  in_flight_loads.remove(model)

  // 5. SSOT update + CB
  match &result:
    Ok(_) → vram_pool.record_loaded(provider_id, model, ...); circuit_breaker.record_success()
    Err(_) → circuit_breaker.record_failure(FailureReason::LoadFailed)

  return result
```

### §6.2a `LoadInFlight` + `FailureReason` — exact types

**`crates/veronex/src/infrastructure/outbound/ollama/lifecycle.rs`** — types:

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;
use tokio::sync::{Notify, OnceCell};

use crate::domain::errors::LifecycleError;
use crate::application::ports::outbound::model_lifecycle::LifecycleOutcome;

/// Per-(provider, model) load-in-flight slot. Concurrent ensure_ready calls for
/// the same model share this slot via the in_flight_loads DashMap.
pub(super) struct LoadInFlight {
    pub started_at: Instant,
    /// Notifies all waiters when the load completes (success or failure).
    pub notify: Arc<Notify>,
    /// Updated every LIFECYCLE_PROGRESS_POLL by the probe runner.
    /// Stall detection compares Instant::now() against this.
    pub last_progress_at: Arc<AtomicU64>,
    /// Set exactly once when probe terminates. OnceCell allows readers to clone.
    pub result: OnceCell<Result<LifecycleOutcome, LifecycleError>>,
}

impl LoadInFlight {
    pub fn new() -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        Self {
            started_at: Instant::now(),
            notify: Arc::new(Notify::new()),
            last_progress_at: Arc::new(AtomicU64::new(now_ms)),
            result: OnceCell::new(),
        }
    }
}
```

**`crates/veronex/src/infrastructure/outbound/ollama/circuit_breaker.rs`** — append:

```rust
/// Distinguishes failure phases for observability without splitting the CB itself.
/// CB trip threshold counts ALL failures (sum across reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    LoadFailed,            // ensure_ready / probe path failed
    InferenceFailed,       // stream_tokens errored mid-stream
    StreamError,           // network / parse error in stream
    ProviderUnreachable,   // health check fail
}

// Extend existing record_failure to accept reason:
impl ProviderCircuitBreaker {
    pub fn record_failure(&self, reason: FailureReason) {
        // existing logic +
        self.failure_reason_counter.with_label_values(&[reason.as_label()]).inc();
        // existing trip threshold logic unchanged
    }
}
```

### §6.2b ollama `/api/generate` zero-prompt probe — exact request/response

The probe re-uses the pattern from `crates/veronex/src/infrastructure/outbound/ollama/preloader.rs:35-60` (verified 2026-04-28):

**Request body**:
```json
{
  "model": "qwen3-coder-next-200k:latest",
  "prompt": "",
  "num_predict": 0,
  "keep_alive": "30m"
}
```

Note: `keep_alive` is a string ("30m") for time-bounded retention OR `-1` for no-expiry preload. Lifecycle uses "30m" to align with project memory `low_power_ollama_lifecycle` (homelab burst window). Startup preloader keeps `-1` for warm-set models.

**Success criterion** — HTTP 200 status. Body example:
```json
{"model":"...","created_at":"...","response":"","done":true,"done_reason":"length",...,"load_duration":163_671_378_039,...}
```

We do NOT need to parse the response body — HTTP 200 is sufficient because:
- `num_predict: 0` always returns immediately after load (no tokens to generate)
- `done: true` is asserted by ollama for any completed request
- `load_duration` is observability data only (recorded into VramPool stats)

**Failure criterion** — non-2xx HTTP status OR connection error. Map to `LifecycleError::ProviderError`.

### §6.2c Stall detection algorithm — exact pseudo-code

Runs as a tokio task in parallel with the probe HTTP call. Both share the `LoadInFlight` slot.

```
spawn_stall_monitor(slot, model):
  loop:
    sleep LIFECYCLE_PROGRESS_POLL                    // 5s
    if slot.result is set:
      return                                          // probe finished, exit monitor
    
    let now_ms = unix_ms_now()
    let last_progress = slot.last_progress_at.load(SeqCst)
    let no_progress_ms = now_ms - last_progress
    
    // Check ollama-side progress via /api/ps
    let ps = fetch_ps()  // GET /api/ps (5s reqwest timeout)
    let model_now_listed = ps.models.contains(model)
    
    if model_now_listed:
      // ollama has at least started loading — update progress
      slot.last_progress_at.store(now_ms, SeqCst)
      no_progress_ms = 0
    
    if no_progress_ms >= LIFECYCLE_STALL_INTERVAL_MS: // 60_000
      slot.result.set(Err(Stalled { last_progress_ms: no_progress_ms }))
      slot.notify.notify_waiters()
      return  // abort; probe HTTP will eventually return but we've already errored
```

Race semantics:
- If probe finishes before stall fires → `slot.result` set by probe, monitor exits via early return
- If stall fires first → `slot.result` set by monitor; probe HTTP keeps running but its result will be ignored (`set` is idempotent — first writer wins)

### §6.2d `preloader.rs` extraction — current → new

Current (`preloader.rs:34-78`): `pub async fn preload_model(client, base_url, model, provider_id, vram_pool, num_parallel) -> bool` — does probe + VramPool side-effects + logging.

After Tier B refactor:
- Create `pub(super) async fn probe_load(client, base_url, model, keep_alive)` in `lifecycle.rs` — pure HTTP call, no VramPool side-effects.
- `preload_model()` in `preloader.rs` calls `probe_load(client, base_url, model, "-1")` then handles VramPool (existing logic).
- `OllamaAdapter::ensure_ready` calls `probe_load(client, base_url, model, "30m")` directly + handles VramPool/CB itself.

Both call sites pass different `keep_alive` strings.

### §6.3 Constants (add to `lifecycle.rs`)

```rust
const LIFECYCLE_LOAD_TIMEOUT: Duration = Duration::from_secs(600);   // hard cap (200K = 163s measured)
const LIFECYCLE_STALL_INTERVAL: Duration = Duration::from_secs(60);  // no-progress abort
const LIFECYCLE_PROGRESS_POLL: Duration = Duration::from_secs(5);    // /api/ps progress check (during probe only)
const LIFECYCLE_KEEP_ALIVE: &str = "30m";                            // burst window
```

### §6.4 Acceptance criteria

- [ ] `cargo test -p veronex --lib infrastructure::outbound::ollama::lifecycle::` — 9 unit tests pass:
  - `already_loaded_fast_path`
  - `cold_load_completion`
  - `coalesce_concurrent_same_model`
  - `coalesce_count_eq_request_count_minus_one`
  - `stall_detection_triggers_at_60s_no_progress`
  - `hard_timeout_at_600s`
  - `provider_error_propagates`
  - `circuit_breaker_open_short_circuits`
  - `thermal_throttled_short_circuits`
- [ ] `cargo test -p veronex --lib infrastructure::outbound::capacity::vram_pool::` — record_loaded / is_loaded / record_evicted covered
- [ ] No production behavior change (still wired via flag in §7)
- [ ] Mock ollama HTTP server (`wiremock`) integration test for end-to-end probe flow

### §6.5 Tests required (Tier B)

| Test | Assertion |
| ---- | --------- |
| `already_loaded_fast_path` | VramPool says loaded → return AlreadyLoaded < 1ms, NO HTTP call to ollama |
| `cold_load_completion` | mock probe returns 200 OK after 50ms → return LoadCompleted{duration_ms ≈ 50} |
| `coalesce_concurrent_same_model` | spawn 5 concurrent ensure_ready(M); mock returns once at 100ms → 1× LoadCompleted, 4× LoadCoalesced |
| `coalesce_count_eq_request_count_minus_one` | observed via metrics counter |
| `stall_detection_triggers_at_60s_no_progress` | mock probe pending forever, /api/ps shows no progress → Stalled within 60-65s |
| `hard_timeout_at_600s` | mock probe pending forever, /api/ps shows progress every 30s → LoadTimeout at 600-605s |
| `provider_error_propagates` | mock returns 502 → Err(ProviderError) |
| `circuit_breaker_open_short_circuits` | CB pre-set to open → Err(CircuitOpen) without HTTP call |
| `thermal_throttled_short_circuits` | thermal_map says throttled → Err(ProviderError("thermal throttle")) without HTTP |
| `vram_pool_record_loaded_updates_aimd_epoch` | record_loaded triggers `aimd_epoch_started_at` reset (per capacity.md Cold Start) |

### §6.6 Resume note

If §5 done but §6 partial: check `feat/ollama-lifecycle` branch. Run `cargo check -p veronex` then `cargo test -p veronex --lib infrastructure::outbound::ollama::lifecycle`. Compare passing tests vs §6.4 / §6.5 list to find resume point.

---

## §7 Tier C — Runner integration + feature flag

> Goal: `runner.rs` orchestrates Phase 1 → Phase 2. Behind feature flag for safe rollout.
> Estimate: ~300 LoC. Branch: `feat/runner-lifecycle-phase`. Blocked on §6.

### §7.1 Files to create / modify

| File | Action | Purpose |
| ---- | ------ | ------- |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | MODIFY | invoke `provider.ensure_ready(model)` before `stream_tokens`; flag-gated |
| `crates/veronex/src/infrastructure/inbound/http/state.rs` | MODIFY | add `lifecycle: Arc<dyn ModelLifecyclePort>` (or extend existing provider trait object to also be lifecycle) |
| `crates/veronex/src/main.rs` | MODIFY | wire lifecycle in composition root; read `MCP_LIFECYCLE_PHASE` env var (default `off` → on after live verification) |
| `crates/veronex/src/domain/constants.rs` | MODIFY | add `MCP_LIFECYCLE_PHASE_FLAG_ENV: &str = "MCP_LIFECYCLE_PHASE"` |
| `clusters/home/values/veronex-dev-values.yaml` (platform-gitops) | MODIFY | set `MCP_LIFECYCLE_PHASE=on` post-verification |

### §7.1a runner.rs — exact insertion point

Current `crates/veronex/src/application/use_cases/inference/runner.rs:497-499`:

```rust
    // stream_tokens must be called BEFORE taking messages — the adapter reads
    // job.messages to decide whether to call stream_chat (with tools) or stream_generate.
    let mut stream = provider.stream_tokens(&job);
```

Tier-C inserts ensure_ready BEFORE the `stream_tokens` call, after the OTel emit at line 488 (`emit_status_event`). Result:

```rust
    // ── Phase 1: Lifecycle (ensure model loaded) ──────────────────────
    if state.config.mcp_lifecycle_phase_enabled {
        match provider.ensure_ready(&job.model_name).await {
            Ok(outcome) => {
                state.vram_pool.record_loaded(provider.id(), &job.model_name, &outcome);
                tracing::info!(
                    %job.id, provider_id = %provider.id(), model = %job.model_name,
                    ?outcome, "lifecycle.ensure_ready"
                );
            }
            Err(e) => {
                tracing::warn!(
                    %job.id, provider_id = %provider.id(), model = %job.model_name,
                    error = %e, "lifecycle.ensure_ready failed"
                );
                // Mark job failed; cleanup; no inference attempt.
                mark_job_failed(&jobs, &job_repo, uuid, &format!("lifecycle: {e}")).await?;
                return Ok(None);
            }
        }
    }

    // ── Phase 2: Inference (stream_tokens) ────────────────────────────
    // stream_tokens must be called BEFORE taking messages — the adapter reads
    // job.messages to decide whether to call stream_chat (with tools) or stream_generate.
    let mut stream = provider.stream_tokens(&job);
```

When flag is OFF: `ensure_ready` is skipped; behavior identical to pre-Tier-C (existing implicit auto-load preserved).

### §7.1b Feature flag pattern

Veronex uses `domain::constants` for env var names + `infrastructure::inbound::http::config` for parsing. Pattern (existing example in `domain/constants.rs`):

```rust
pub const MCP_LIFECYCLE_PHASE_FLAG_ENV: &str = "MCP_LIFECYCLE_PHASE";
pub const MCP_LIFECYCLE_PHASE_DEFAULT: bool = false;
```

In `main.rs` composition root (after `AppState::new(...)`, before HTTP server bind):

```rust
let mcp_lifecycle_phase_enabled = std::env::var(MCP_LIFECYCLE_PHASE_FLAG_ENV)
    .ok()
    .and_then(|v| match v.to_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    })
    .unwrap_or(MCP_LIFECYCLE_PHASE_DEFAULT);

tracing::info!(
    enabled = mcp_lifecycle_phase_enabled,
    env = %MCP_LIFECYCLE_PHASE_FLAG_ENV,
    "mcp lifecycle phase"
);

// Carried in AppState via Config struct:
let config = AppConfig {
    mcp_lifecycle_phase_enabled,
    // ... existing fields
};
```

Helm values (`clusters/home/values/veronex-dev-values.yaml`) — set after verification:
```yaml
api:
  env:
    - name: MCP_LIFECYCLE_PHASE
      value: "on"  # default off; flip after Tier-C live verification
```

### §7.2 Observability

Tracing spans (via `#[instrument]` or `info_span!`):

```
inference.process_job
├─ lifecycle.ensure_ready { provider_id, model, outcome, duration_ms }
└─ inference.stream_tokens { provider_id, model, tokens_generated }
```

Metrics (Prometheus, exposed via existing OTel pipeline):

| Metric | Type | Labels |
| ------ | ---- | ------ |
| `veronex_lifecycle_ensure_ready_seconds` | histogram | provider_id, model, outcome |
| `veronex_lifecycle_load_failed_total` | counter | provider_id, model, reason |
| `veronex_lifecycle_coalesced_total` | counter | provider_id, model |
| `veronex_inference_first_token_seconds` | histogram | provider_id, model |

### §7.3 Acceptance criteria

- [ ] Feature flag `MCP_LIFECYCLE_PHASE=off` (default): runner behavior identical to pre-Tier-C — NO regression on live tests
- [ ] Feature flag `MCP_LIFECYCLE_PHASE=on`: live verification on dev cluster (§7.4) all pass
- [ ] Tracing spans visible in Tempo / log output
- [ ] Metrics scraped by Prometheus
- [ ] Bridge phased timeouts (PR #90) unchanged

### §7.4 Live verification on dev cluster

Run these in order. Mark each [x] when verified.

```bash
# Setup
KUBECTL_NS=app-veronex
TOKEN=$(curl -s -X POST https://veronex-api-dev.verobee.com/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"test-3","password":"test1234!"}' -i \
  | grep -i 'set-cookie: veronex_access_token=' | head -1 \
  | sed -E 's/.*veronex_access_token=([^;]+);.*/\1/')

# Step 1 — Trigger model unload (force cold scenario)
kubectl exec -n app-ai-platform ollama-0 -- ollama stop qwen3-coder-next-200k:latest

# Step 2 — Send request (cold path)
time curl -sS --max-time 240 -X POST https://veronex-api-dev.verobee.com/v1/chat/completions \
  -H "Cookie: veronex_access_token=$TOKEN" -H "Content-Type: application/json" \
  -d '{"model":"qwen3-coder-next-200k:latest","messages":[{"role":"user","content":"hi"}],"use_mcp":true,"stream":false}' \
  | jq -r '.choices[0].message.content' | head -3

# EXPECTED:
#   real    ~165s (matches measured 163s + overhead)
#   content: non-empty response (not error / empty)
#   no "client connection closed before server finished loading" in ollama logs
#   tracing span "lifecycle.ensure_ready" duration ≈ 163s, "inference.stream_tokens" < 5s

# Step 3 — Concurrent same-model coalescing
for i in 1 2 3; do
  curl -sS --max-time 240 -X POST https://veronex-api-dev.verobee.com/v1/chat/completions \
    -H "Cookie: veronex_access_token=$TOKEN" -H "Content-Type: application/json" \
    -d '{"model":"qwen3-coder-next-200k:latest","messages":[{"role":"user","content":"hi"}],"use_mcp":true,"stream":false}' &
done
wait

# EXPECTED:
#   all 3 succeed
#   metric `veronex_lifecycle_coalesced_total{model="qwen3-coder-next-200k:latest"}` increased by ≥2

# Step 4 — Warm path
time curl ... # same request, immediately
# EXPECTED:
#   real    < 5s
#   tracing "lifecycle.ensure_ready" outcome = AlreadyLoaded, duration_ms < 50

# Step 5 — Provider error path
kubectl scale deploy/ollama -n app-ai-platform --replicas=0
time curl ... # within 10s should fail with LifecycleError
kubectl scale deploy/ollama -n app-ai-platform --replicas=1

# EXPECTED:
#   LifecycleError::ProviderError surfaced to client
#   provider_circuit_breaker.failure_count incremented with reason=LoadFailed
```

### §7.5 Resume note

If §6 done but §7 partial: check `feat/runner-lifecycle-phase` branch. Verify §7.1 file changes present. Run §7.4 live tests. If a step fails, fix that file and re-run.

---

## §8 Post-implementation: CDD-sync (per `.add/doc-sync.md`)

> Goal: bring `docs/llm/` in sync with new code. Rule: **code is SSOT, docs describe code**.
> Branch: `docs/cdd-lifecycle`. Blocked on §7 live verification pass.

### §8.1 CDD updates

| File | Action | Anchor |
| ---- | ------ | ------ |
| `docs/llm/inference/mcp.md` | Insert **before** existing `### Verification (2026-04-28)` heading at line 158 a new section `### Phase 1 Lifecycle / Phase 2 Inference`. Update verification table with Tier-C results. | line 149 (after `### YQL Contract`), insert at line 158 |
| `docs/llm/policies/architecture.md` | In `## Multi-Provider Routing` section, insert after the `→ score providers → pick highest` line a `─ Phase 1 lifecycle / Phase 2 inference` subsection | search for "Multi-Provider Routing" anchor |
| `docs/llm/policies/patterns.md` | Append new `## Lifecycle Port Pattern` section at end (port + adapter + runner orchestration) | EOF append |
| `docs/llm/inference/job-lifecycle.md` | Insert ensure_ready step into existing job-state-machine doc | search "Phase X" anchor and add Phase between "Submitted" and "Running" |
| `docs/llm/inference/job-lifecycle-impl.md` | Reflect runner.rs orchestration in implementation walkthrough | search "runner" anchor |
| `docs/llm/inference/capacity.md` | VramPool record_loaded / record_evicted entry-point clarification | search "VramPool" anchor |
| `docs/llm/components/ollama.md` (if exists) | Lifecycle adapter section | EOF append |
| `.ai/components.md` | NO change (internal port, no new visible component) | — |

### §8.2 Token-optimization compliance (`docs/llm/policies/token-optimization.md`)

- Tables > prose
- No emoji except `✓`/`✗`
- ≤ H3
- No filler phrases ("please note", "the following", "as mentioned")
- Layer 2 doc body ≤ 200 lines (each)

### §8.3 Acceptance criteria

- [ ] `grep -rn 'COLLECT_ROUND_TIMEOUT\|stream_tokens.*load' docs/llm/` returns nothing (stale references purged)
- [ ] All new code paths documented in CDD
- [ ] Pre-PR grep: `grep -iE 'please note|the following|as mentioned' docs/llm/` returns nothing
- [ ] `docs/llm/README.md` index updated if new file added

---

## §9 Post-implementation: Flow-sync (per `.add/doc-sync.md` step 8)

> Goal: update flow diagrams when control flow changes.
> Branch: same as §8 (`docs/cdd-lifecycle`).

### §9.1 Flow updates

| File | Change | Anchor |
| ---- | ------ | ------ |
| `docs/llm/flows/inference.md` | After `vram_pool.reserve(provider_id, model)` line, insert lifecycle phase | search for "vram_pool.reserve" |
| `docs/llm/flows/mcp.md` | Add lifecycle ensure_ready in MCP loop diagram | inside `## Bridge run_loop` section |
| `docs/llm/flows/model-lifecycle.md` | NEW — standalone state-transition diagram | new file |
| `docs/llm/flows/README.md` | Append `model-lifecycle` row | end of file |

### §9.1a Existing ASCII format reference (pattern in flows/inference.md)

```
Client
  │  POST /v1/chat/completions  (Bearer API Key or session JWT)
  ▼
InferCaller middleware
  ├── API Key path  → BLAKE2b hash → DB lookup → RPM/TPM rate limit check
  └── JWT path      → verify HS256 → extract account_id
  │
  ▼
openai_handlers::chat_completions()
```

Conventions:
- `│` `▼` for vertical flow
- `├──` `└──` for branches
- Use indented arrows after the line that triggers them
- Section dividers via `---` (markdown)
- File / line / function references in code-fence ticks

### §9.1b New `flows/model-lifecycle.md` skeleton

```
# Model Lifecycle

> **Last Updated**: 2026-XX-XX (filled in at PR time)

## State Transitions

```
NotLoaded ──► Loading ──► Loaded
   ▲           │           │
   │           ▼           ▼
   └── Evicted ◄─── Failed (retry_after)
                     │
                     └── Loading (retry)
```

## ensure_ready Flow

```
ensure_ready(model)
  │
  ├─ provider_circuit_breaker.guard()? ── Err ──► CircuitOpen
  │
  ├─ thermal_map.is_throttled()?       ── Yes ──► ProviderError("thermal throttle")
  │
  ├─ vram_pool.is_loaded(P, M)?         ── Yes ──► AlreadyLoaded (instant)
  │
  ├─ in_flight_loads.contains(M)?       ── Yes ──► wait notify ──► LoadCoalesced{waited_ms}
  │
  ├─ acquire LoadInFlight slot
  │   ├─ spawn stall_monitor
  │   ├─ POST /api/generate {prompt:"", num_predict:0, keep_alive:"30m"}
  │   │     ├─ 200 OK ──► slot.result = LoadCompleted{duration_ms}
  │   │     └─ Err    ──► slot.result = ProviderError(...)
  │   ├─ stall fires (no progress 60s) ──► slot.result = Stalled
  │   └─ hard timeout (600s)             ──► slot.result = LoadTimeout
  │
  ├─ slot.notify.notify_waiters()
  ├─ in_flight_loads.remove(M)
  │
  ├─ on Ok: vram_pool.record_loaded(P, M) + circuit_breaker.record_success()
  └─ on Err: circuit_breaker.record_failure(FailureReason::LoadFailed)
```

## Background sync

`sync_loop` (30s) polls each provider's `/api/ps`:
- For each loaded model in response → `vram_pool.record_loaded` (idempotent)
- For each model present-then-missing → `vram_pool.record_evicted(KeepAliveExpired)`

This is the SSOT refresh path. `ensure_ready` reads the SSOT but does not write outside its own load events.
```

### §9.1c Update existing `flows/inference.md`

Find anchor: `vram_pool.reserve(provider_id, model)       ← acquire KV permit`. Insert AFTER:

```
       ├── vram_pool.reserve(provider_id, model)       ← acquire KV permit
       │
       ├── Phase 1: lifecycle.ensure_ready()           ← see flows/model-lifecycle.md
       │     ├── thermal + circuit breaker re-check
       │     ├── vram_pool.is_loaded? → AlreadyLoaded
       │     └── otherwise probe + record_loaded
       │
       ├── Phase 2: provider.stream_tokens(&job)
       │
```

### §9.2 Acceptance criteria

- [ ] Flow diagrams use ASCII art consistent with existing flows in `docs/llm/flows/`
- [ ] State transitions per `domain/value_objects/model_instance_state.rs` reflected
- [ ] Coalescing semantics shown in `model-lifecycle.md`
- [ ] `docs/llm/flows/README.md` lists `model-lifecycle.md`

---

## §10 Out of Scope (deliberate)

- **Predictive lifecycle / autonomous preload** — separate SDD `inference-lifecycle-autonomous.md`. Requires demand-curve learning, conversation pattern detection, idle-prediction. Significant scope (>3000 LoC).
- **Cross-pod lifecycle coordination** — current ollama idempotency is sufficient at scale; revisit if 10K provider scale shows duplicate-load thrashing.
- **Operator-initiated preload endpoint** — `POST /v1/admin/models/{model}/preload`. Defer to S15 if needed.
- **Distributed sccache for model weights** — orthogonal concern.

---

## §11 Risks & Mitigations

| Risk | Mitigation |
| ---- | ---------- |
| Feature flag bug breaks live inference | Default `off`. dev cluster validation before prod default-on. |
| `ensure_ready` adds latency on warm path | VramPool DashMap O(1) lookup; mocked in §6.4 to be < 1ms |
| Thermal/CB race window between dispatch and ensure_ready | Defensive re-check inside `ensure_ready` per §6.2 |
| Single CB accumulates load + inference failures, trips faster | Acceptable — CB is per-provider safety; tag separation lets operator tune later |
| Stall detection 60s threshold too aggressive | Tunable constant; defaults conservative; observability lets operators adjust |
| ollama itself returns success but model is actually broken | Out of scope — provider health beyond `/api/generate` 200 OK is ollama's responsibility |

---

## §12 References

- Live measurement (2026-04-28): `qwen3-coder-next-200k:latest` `load_duration` = 163,671 ms (AI Max+ 395 / 128 GB / ROCm 7.2)
- veronex#90 (commit `22887f1`): bridge phased timeout (defense layer)
- Vision: `docs/llm/policies/architecture.md` §Vision (8 principles)
- Hexagonal: `.ai/architecture.md` + `docs/llm/policies/architecture.md`
- VramPool SSOT: `docs/llm/inference/capacity.md`
- Project memory: `~/.claude/projects/-home-beegy-workspace-labs-veronex/memory/project_low_power_ollama_lifecycle.md`
- ollama docs: `OLLAMA_LOAD_TIMEOUT` default, `/api/ps`, `/api/generate` zero-token probe pattern
- Strix Halo benchmarks: `https://forum.level1techs.com/t/strix-halo-ryzen-ai-max-395-llm-benchmark-results/233796`
- Inngest agent loop: `https://www.inngest.com/docs/ai-patterns/agent-tool-loops`
- Temporal AI cookbook: `https://docs.temporal.io/ai-cookbook/agentic-loop-tool-call-openai-python`
