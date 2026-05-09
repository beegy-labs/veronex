# Model Lifecycle (Phase 1 ↔ Phase 2 SoD)

> **Last Updated**: 2026-05-10

`runner::run_job` still splits provider work into two phases, but the
heavy "Phase 1 probe loop" that existed in the Ollama era is gone — with
llama-server running one model per process, lifecycle = process
lifecycle. The probe/coalesce/`/api/ps` machinery moved to
`ProcessManager`; the `ModelLifecyclePort` survives as a thin trait so
the dispatcher keeps its phase boundary.

## State Machine — `ModelInstanceState`

```
NotLoaded ──ensure_running──▶ Loading ──/health 200──▶ Loaded
    ▲                            │                       │
    │                            │ spawn / health fails  │ stop (idle reap, manual)
    │                            ▼                       │
    └─── ProcessManager.stop  Failed                  Evicted
                                 │ (admin alert)         │
                                 │                       └── ensure_running ──▶ Loading
                                 ▼
                              (op alert)
```

| State | Meaning |
|-------|---------|
| `NotLoaded` | no llama-server process for `(provider, model)` yet |
| `Loading` | `ProcessManager.ensure_running` in flight; concurrent callers wait on the per-provider `Mutex` |
| `Loaded` | process running, `/health` 200, AIMD window registered |
| `Failed` | spawn or health probe failed; `RunningProcess.state = Failed`; admin must clear |
| `Evicted` | `IdleManager` or admin called `stop`; next request re-enters `Loading` |

Transitions are driven by `ProcessManager` state in
`infrastructure/outbound/process_manager/manager.rs::ProcessState`.

## Phase 1 — `ensure_ready` (lifecycle, thin)

```
runner::run_job (post-VRAM reserve, pre-stream_tokens)
  │
  ├── [MCP_LIFECYCLE_PHASE=off]?  skip — process is already up
  │                                (ProcessManager warmed it on dispatch)
  │
  └── [MCP_LIFECYCLE_PHASE=on]?
        │
        ▼
   provider.ensure_ready(model)        ← LlmProviderPort super-trait
        │
        ├── LlamaServerAdapter::ensure_ready
        │     └── self.health() → SlotStatus
        │           ├── 200 + ok    → LifecycleOutcome::AlreadyLoaded
        │           ├── 200 + busy  → LifecycleError::ProviderError
        │           └── err / 5xx   → LifecycleError::ProviderError
        │
        └── GeminiAdapter::ensure_ready
              └── always Ok(AlreadyLoaded) — cloud, no local lifecycle
```

The legacy probe — `POST /api/generate { num_predict:0, keep_alive }`
plus a `GET /api/ps` poller plus stall detection plus a `LoadInFlight`
DashMap slot — was removed when veronex stopped speaking the Ollama API.
The equivalent of the old "warm" guarantee is now upstream of dispatch:
`ProcessManager.ensure_running` already spawned the process and polled
`/health` until it succeeded before any job was claimed for it.

### `LifecycleOutcome` (current)

| Variant | When |
|---------|------|
| `AlreadyLoaded` | `/health` returned 200 + ready slot |
| `LoadCompleted` | reserved on the trait; not emitted by the current adapters (`ProcessManager` owns spawn timing) |
| `LoadCoalesced` | reserved on the trait; coalescing now happens at the `ProcessManager` Mutex level |

### `LifecycleError`

| Variant | When |
|---------|------|
| `LoadTimeout(s)` | reserved on the trait; `ProcessManager.ensure_running` enforces its own timeouts on `NodeClient.health` poll |
| `Stalled(s)` | reserved on the trait; not emitted in the simplified adapter |
| `ProviderError(msg)` | `/health` failed or returned non-ready status |
| `CircuitOpen` | per-provider CB rejected before HTTP |
| `ResourcesExhausted(msg)` | VramPool refused reservation (defensive — runner already gated) |

## Phase 2 — `stream_tokens` (inference)

Unchanged: streams tokens from `POST /v1/chat/completions` to the SSE
pipeline, broadcasts status events, finalizes the job in `finalize_job()`.
Phase 1's success guarantees a live process; first-token latency is
bounded by inference, not load.

## Replaces / Migration Notes

| Pre-migration concept | Where it went |
|-----------------------|---------------|
| `LoadInFlight` DashMap + leader/follower coalescing | Per-provider `Mutex` inside `ProcessManager` |
| `POST /api/generate` zero-prompt warmup probe | Process spawn via `NodeClient.spawn(model, port)` |
| `GET /api/ps` size-vram poller | `NodeClient.health(port)` polled until 200 |
| `last_progress_at` sentinel-zero stall detector | `ProcessState::Failed` if `health` poll exceeds `ensure_running` deadline |
| `model_ctx` Valkey hot-path cache for num_ctx alignment | Modelfile registry row's `context_length` (Phase 2 SSOT) |

The `MCP_LIFECYCLE_PHASE` flag still gates whether `runner` calls
`ensure_ready` explicitly. With the simplified adapter the call is
near-free (one `/health` round-trip), and the bridge phased timeouts
remain the safety net.

## Files

| File | Purpose |
|------|---------|
| `domain/value_objects.rs` | `ModelInstanceState`, `EvictionReason` |
| `domain/errors.rs` | `LifecycleError` |
| `application/ports/outbound/model_lifecycle.rs` | `ModelLifecyclePort` trait + `MockLifecycle` |
| `application/ports/outbound/inference_provider.rs` | `LlmProviderPort` super-trait + blanket impl |
| `infrastructure/outbound/llama_server/adapter.rs` | `LlamaServerAdapter::ensure_ready` (now a thin `/health` check) |
| `infrastructure/outbound/llama_server/health.rs` | `SlotStatus` parsing |
| `infrastructure/outbound/process_manager/manager.rs` | `ProcessManager::ensure_running` (real lifecycle work) |
| `infrastructure/outbound/process_manager/agent_client.rs` | `NodeClient` trait — `spawn`, `health`, `stop` |
| `infrastructure/outbound/gemini/adapter.rs` | no-op cloud `impl ModelLifecyclePort` |
| `application/use_cases/inference/runner.rs` | Phase 1 block before `stream_tokens`, flag-gated |
| `bootstrap/background.rs` | parses `MCP_LIFECYCLE_PHASE` env, wires `ProcessManager` + flag |
