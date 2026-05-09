# Process Manager (llama-server lifecycle)

> **Last Updated**: 2026-05-10

Replaces the legacy Ollama-era placement planner. The new model is
**lazy spawn → idle reap → AIMD admission**, not eager preload + LRU evict.

## Overview

| Component | File | Role |
|-----------|------|------|
| `ProcessManager` | `infrastructure/outbound/process_manager/manager.rs` | Owns one `NodeClient` + `PortPool` per registered llm node; serializes spawns per provider via `Mutex<()>`; in-memory `provider_id → RunningProcess` map |
| `IdleManager` | `infrastructure/outbound/process_manager/idle_manager.rs` | Background tick (default 30s) that stops processes whose `idle_for(now) >= ttl` |
| `ActivityTracker` | `infrastructure/outbound/process_manager/activity_tracker.rs` | RAII `RequestGuard` — bumps in-flight on start, decrements on drop; SSOT for "is provider idle?" |
| `AimdAdmission` | `infrastructure/outbound/capacity/admission.rs` | Per-provider RAII window gate; rejects (returns `None`) when full |
| `PortPool` | `infrastructure/outbound/process_manager/port_pool.rs` | Per-node port allocator; `PortLease` released on drop |
| `NodeClient` | `infrastructure/outbound/process_manager/agent_client.rs` | Trait — production = `HttpNodeClient`, tests = `StubNodeClient` |

## Lifecycle States

```
ProcessState ::= Loading → Warming → Ready
                                  ↘ Failed (admin must clear)
                Ready → Stopping (idle reap or explicit stop)
```

Defined on `manager::ProcessState`. The router only dispatches to `Ready`.

## ensure_running

Idempotent spawn. The flow:

```
ensure_running(provider_id, model_id):
  acquire per-provider Mutex (one in-flight spawn per provider)
  if already Ready → return
  if Loading/Warming  → caller waits on the same Mutex
  else:
    PortLease ← PortPool.allocate()
    NodeClient.spawn(model_id, port) → agent_handle
    transition Loading → Warming
    poll NodeClient.health(port) until 200 or timeout
    on success: transition → Ready
                ActivityTracker.register
                AimdRegistry.register (default window from AIMD policy)
    on failure: transition → Failed; release PortLease
```

The Mutex is per-provider, not global — different providers spawn in parallel.

## Idle Reap

`IdleManager.tick()` runs every `idle_check_interval_secs`:

```
for p in ProcessManager.list_running():
    ttl = override_fn(p.provider_id)
        ?? system_settings.llama_server.idle_ttl_seconds
        ?? 60
    if ttl == 0: continue                       # 0 = never reap
    if ActivityTracker.idle_for(p.provider_id, now) >= ttl:
        ProcessManager.stop(p.provider_id)
```

`stop()` is idempotent and runs side-effects (`ActivityTracker.unregister`,
`AimdRegistry.unregister`, `PortLease.drop`) before the network call so a
slow/failing agent doesn't leak local state.

## AIMD Admission

`AimdAdmission.acquire(provider_id)` returns `Option<AdmissionGuard>`:

| Outcome | Caller behavior |
|---------|-----------------|
| `Some(guard)` | Run the request; guard decrements on drop |
| `None` (window full) | 503 — dispatcher tries the next candidate, client retries |

The window size moves per `AimdController` (Concur α=2/β=0.5, MARS dual-pressure,
TokenScale SLO) — see `inference/capacity.md` for the controller details.

## Wiring

```
bootstrap/background.rs:
  ProcessManager::new(activity_tracker, aimd_registry)
  IdleManager::new(activity_tracker, manager, settings_repo, override_fn)
  spawn IdleManager.run(Duration::from_secs(30), shutdown_token)
```

`AppState` holds `Arc<ProcessManager>` and `AimdAdmission` so the dispatcher
+ inference handlers can call `ensure_running` before routing and
`acquire` before each request.

## Replaces

| Legacy concept (Ollama-era) | New mechanism |
|-----------------------------|---------------|
| `placement_planner.rs` 5s tick — scale-out + preload + evict + scale-in | `ensure_running` (lazy on first request) + `IdleManager` (TTL-based reap) |
| LRU eviction by `OllamaModelManager` | `IdleManager` per-provider TTL — one model per llama-server process by design |
| `set_online()` heartbeat from `veronex-agent` | `NodeClient.health()` probe during `ensure_running`; activity-driven heartbeat via `ActivityTracker` |
| Capacity analyzer batch sync (`capacity::analyzer`) | Per-node probe at registration; AIMD window controller in-process |
