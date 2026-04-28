# Model Lifecycle (Phase 1 ↔ Phase 2 SoD)

> **Last Updated**: 2026-04-28

`runner::run_job` splits provider work into two distinct phases. Phase 1
(`ensure_ready`) is observable in its own span/metric; Phase 2 (`stream_tokens`)
runs only after Phase 1 reports success. SDD: `.specs/veronex/history/inference-lifecycle-sod.md` (archived 2026-04-28 after live verify).

---

## State Machine — `ModelInstanceState`

```
NotLoaded ──ensure_ready────▶ Loading ──probe completes──▶ Loaded
    ▲                            │                           │
    │                            │ probe fails               │ evict / KV reset
    │                            ▼                           │
    └────── reload ─────────  Failed                       Evicted
                                 │ (terminal until restart)   │
                                 │                            └── ensure_ready ──▶ Loading
                                 ▼
                              (op alert)
```

| State | Meaning |
|-------|---------|
| `NotLoaded` | model never probed on this provider, or evicted from VRAM |
| `Loading` | a probe is in flight; concurrent calls coalesce on the same `LoadInFlight` slot |
| `Loaded` | weights + KV cache resident, `keep_alive` window active |
| `Failed` | probe surfaced `LifecycleError`; CB increments; ops paged |
| `Evicted` | VRAM reclaimed (manual or thermal); next request re-enters `Loading` |

Transitions are validated by `ModelInstanceState::can_transition_to` (`domain/value_objects.rs`).

---

## Phase 1 — `ensure_ready` (lifecycle)

```
runner::run_job (post-VRAM reserve, pre-stream_tokens)
  │
  ├── [MCP_LIFECYCLE_PHASE=off]? skip — implicit auto-load via stream_tokens
  │
  └── [MCP_LIFECYCLE_PHASE=on]?
        │
        ▼
   provider.ensure_ready(model)        ← LlmProviderPort super-trait method
        │
        ├── 1. VramPool SSOT check (OllamaAdapter)
        │     └── loaded_model_names contains(provider_id, model)?
        │           ├── YES → LifecycleOutcome::AlreadyLoaded   (no HTTP)
        │           └── NO  → enter coalescing path
        │
        ├── 2. Per-(provider, model) in-flight coalescing
        │     in_flight_loads: DashMap<String, Arc<LoadInFlight>>
        │     ├── slot occupied → wait on Notify → LoadCoalesced{waited_ms}
        │     └── slot empty    → become leader → run probe
        │
        ├── 3. Probe (leader only) — concurrent observers via tokio::select!
        │     ┌── probe_fut       POST /api/generate { num_predict:0,
        │     │                                        keep_alive:"30m" }
        │     │                     reqwest::timeout(LIFECYCLE_LOAD_TIMEOUT)
        │     │                     wins on success → LoadCompleted{duration_ms}
        │     │                     wins on error   → ProviderError
        │     │
        │     ├── ps_poller       GET /api/ps every 5s (MissedTickBehavior::Delay)
        │     │                     entry must satisfy size_vram > 0 AND
        │     │                     names_match(model, entry.name)
        │     │                     (`:latest` defaulting handled both ways)
        │     │                     match → record_progress() (slot.last_progress_at)
        │     │
        │     ├── stall_fut       polls every 5s, no-op while last_progress_at == 0
        │     │                     once first /api/ps confirm: fires when
        │     │                     now − last_progress_at > 60s (post-load HTTP hang)
        │     │
        │     ├── progress_log    info!(model, elapsed_s, first_progress) every 30s
        │     │                     (operator visibility on multi-minute loads)
        │     │
        │     └── hard_cap        sleep LIFECYCLE_LOAD_TIMEOUT + 5s → LoadTimeout
        │
        │     **Probe is NEVER cancelled** when stall/hard_cap wins the select.
        │     Closing the connection mid-load triggers ollama
        │     `client connection closed before server finished loading`
        │     (ollama#8006). Only `reqwest::timeout` may terminate the probe.
        │
        ├── 4. Resolve slot
        │     OnceCell stores Result<LifecycleOutcome, LifecycleError>
        │     waiters wake via Notify and read the OnceCell
        │
        └── 5. On success → VramPool::record_loaded(provider_id, model)
              On error   → LifecycleError variant returned, CB increments
```

### `LifecycleOutcome`

| Variant | When |
|---------|------|
| `AlreadyLoaded` | VramPool reports model present on this provider |
| `LoadCompleted{duration_ms}` | leader probe returned 200 |
| `LoadCoalesced{waited_ms}` | follower joined an in-flight slot and observed leader's success |

### `LifecycleError`

| Variant | When |
|---------|------|
| `LoadTimeout(s)` | hard-cap fired (LIFECYCLE_LOAD_TIMEOUT 600s) |
| `Stalled(s)` | post-`/api/ps`-confirm HTTP hang — `last_progress_at` non-zero **and** gap > LIFECYCLE_STALL_INTERVAL (60s). No-op while sentinel zero (cold-load silent phase) |
| `ProviderError(msg)` | probe HTTP returned non-2xx, or transport error |
| `CircuitOpen` | per-provider CB rejected before HTTP |
| `ResourcesExhausted(msg)` | VramPool refused reservation (defensive — runner already gated) |

### Stall semantics — the sentinel `last_progress_at == 0`

ollama's `POST /api/generate` is a **single request-response** — there is no
streamed progress during silent cold-load (verified [ollama-python#439](https://github.com/ollama/ollama-python/issues/439)).
On 200K-context models a load is 163 s of zero bytes on the wire (measured on
AI Max+ 395 / ROCm 7.2). A naive "stall = N seconds without bytes" detector
therefore misfires on every cold load.

`LoadInFlight::new()` initialises `last_progress_at = 0` (sentinel meaning
"no progress signal observed yet"). `stall_fut` is a no-op while the sentinel
holds; only the `ps_poller` arm above transitions it to a real wall-clock
timestamp on first `/api/ps` confirmation. Stall is then redefined as
**post-load HTTP hang detection** — "ollama claims model loaded but probe HTTP
is not returning". Initial silent-load duration is bounded only by
`reqwest::timeout(LIFECYCLE_LOAD_TIMEOUT)` and the `hard_cap` arm.

Production verified 2026-04-28: `outcome=LoadCompleted { duration_ms: 180862 }`
on a 180 s 200K-context cold load (SDD `.specs/veronex/history/inference-lifecycle-sod.md`
§9.5). Pre-fix code failed the same path at 60 s with `Stalled`.

---

## Phase 2 — `stream_tokens` (inference)

Unchanged from pre-Tier-C: streams tokens to the SSE pipeline, broadcasts
status events, finalizes job in `finalize_job()`. Phase 1's success guarantees
a warm model so first-token latency is bounded by inference, not load.

---

## Flag Behaviour

| `MCP_LIFECYCLE_PHASE` | Pre-`stream_tokens` step | Defense-in-depth |
|-----------------------|--------------------------|------------------|
| `off` (default) | none — implicit auto-load inside `stream_tokens` | bridge phased timeouts (FIRST_TOKEN=240s / IDLE=45s / TOTAL=360s, PR #90) |
| `on` | `provider.ensure_ready(model)` — explicit observable phase | bridge phased timeouts retained as safety net |

Live verification (SDD §7.4) gates the dev/prod flip via
`clusters/home/values/veronex-{dev,prod}-values.yaml::api.env`.

---

## Files

| File | Purpose |
|------|---------|
| `domain/value_objects.rs` | `ModelInstanceState`, `EvictionReason` |
| `domain/errors.rs` | `LifecycleError` |
| `application/ports/outbound/model_lifecycle.rs` | `ModelLifecyclePort`, `LifecycleOutcome`, `MockLifecycle` |
| `application/ports/outbound/inference_provider.rs` | `LlmProviderPort` super-trait + blanket impl |
| `infrastructure/outbound/ollama/lifecycle.rs` | `LoadInFlight`, `probe_load`, `run_probe_with_stall`, constants |
| `infrastructure/outbound/ollama/adapter.rs` | `OllamaAdapter::with_vram_pool`, `impl ModelLifecyclePort` |
| `infrastructure/outbound/gemini/adapter.rs` | no-op cloud `impl ModelLifecyclePort` |
| `infrastructure/outbound/provider_router.rs` | `make_adapter` returns `Arc<dyn LlmProviderPort>` |
| `infrastructure/outbound/provider_dispatch.rs` | `ConcreteProviderDispatch` carries `vram_pool` |
| `application/use_cases/inference/runner.rs` | Phase 1 block before `stream_tokens`, flag-gated |
| `bootstrap/background.rs` | parses `MCP_LIFECYCLE_PHASE` env, injects `vram_pool` + flag |
