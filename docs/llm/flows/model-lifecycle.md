# Model Lifecycle (Phase 1 ↔ Phase 2 SoD)

> **Last Updated**: 2026-04-28

`runner::run_job` splits provider work into two distinct phases. Phase 1
(`ensure_ready`) is observable in its own span/metric; Phase 2 (`stream_tokens`)
runs only after Phase 1 reports success. SDD: `.specs/veronex/inference-lifecycle-sod.md`.

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
        ├── 3. Probe (leader only)
        │     POST /api/generate { model, prompt: "", num_predict: 0,
        │                          keep_alive: "30m" }
        │     monitored by tokio::select! over:
        │       ├── probe future
        │       ├── stall detector  (last_progress_at idle > 60s → poison)
        │       └── hard cap        (LIFECYCLE_LOAD_TIMEOUT 600s)
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
| `LoadTimeout(s)` | hard-cap fired (LIFECYCLE_LOAD_TIMEOUT) |
| `Stalled(s)` | no progress for LIFECYCLE_STALL_INTERVAL (60s) |
| `ProviderError(msg)` | probe HTTP returned non-2xx, or transport error |
| `CircuitOpen` | per-provider CB rejected before HTTP |
| `ResourcesExhausted(msg)` | VramPool refused reservation (defensive — runner already gated) |

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
