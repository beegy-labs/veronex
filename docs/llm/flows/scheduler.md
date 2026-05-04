# Provider Selection & Scheduler Flow

> **Last Updated**: 2026-03-26

---

## Dispatch Path (per job)

```
dispatcher::queue_dispatcher_loop()  — woken by Notify on job submit
  │
  ▼
dequeue next job
  priority: paid(+3000ms) > standard(+1000ms) > free
  tiebreak: age (oldest wins)
  │
  ▼
select_provider(model, provider_type, key_id)
  │
  ├── registry.list_all() filtered by provider_type
  │     └── filter: provider_type matches (no is_active — hard delete removes inactive)
  │
  ├── [API key?] filter by api_key_provider_access allowlist
  │     └── no rows for key → all providers allowed
  │
  ├── filter by global_model_disabled
  │     └── model in disabled list → skip all providers
  │
  ├── filter by provider model selection (is_enabled)
  │     └── model disabled on specific provider → skip that provider
  │
  ├── for each candidate provider:
  │     ├── thermal.get_level(id) == Soft|Hard|Cooldown → skip
  │     ├── circuit_breaker.is_allowed(id) == false → skip
  │     ├── vram_pool.is_pulling(id, model) == true → skip
  │     ├── vram_pool.is_dispatch_blocked(id, model) → skip
  │     └── vram_pool.available_vram_mb(id) < needed → skip
  │
  └── score remaining candidates → pick highest
        score = available_vram_mb
              × thermal.perf_factor(id)   (0.0–1.0, reduced when hot)
              / active_request_count + 1   (load balancing)
  │
  ▼
[no eligible provider?] → job re-queued, 503 if max_wait exceeded
  │
  ▼
vram_pool.reserve(provider_id, model)
  ├── model loaded   → reserve KV cache only
  └── model unloaded → reserve weight_mb + KV cache
  │
  ▼
spawn_job_direct(job_id, provider_id)  → runner::run_job()
  │
  ▼
on completion: vram_pool.release(provider_id, model)
  └── KV cache released; weight stays in VRAM (OLLAMA_KEEP_ALIVE=-1)
```

---

## VRAM Pool States

```
Provider VRAM state machine:

  UNKNOWN (total_mb = 0)
    │  first successful scrape from node-exporter DRM / APU metrics
    ▼
  KNOWN (total_mb > 0)
    │  strict reservation enforced from this point

UNKNOWN mode: available_vram_mb = (max_concurrent - active) × 1024 MB
              routing still works — delegates OOM enforcement to Ollama
```

---

## VRAM Reservation Logic

```
Model already loaded (in /api/ps)?
  YES → reserve KV cache only      = ctx_size × bytes_per_token × 2
  NO  → reserve weight + KV cache  = weight_mb + KV

On request completion:
  release KV only (weight stays — OLLAMA_KEEP_ALIVE=-1)
```

---

## Placement Planner (background, 5s interval)

```
placement_planner::planner_tick()
  │
  ├── Pass 0 (read-only):
  │     ├── list active Ollama providers
  │     ├── compute scale_out_candidates (healthy, VRAM > 0)
  │     ├── fetch model demand from Valkey (demand keys per model)
  │     ├── compute eligible_capacity per model
  │     └── scale_out_needed = models where demand > capacity × 0.80
  │
  ├── Hard Gate Watchdog (SDD §3):
  │     provider in Hard state?
  │       ├── active_requests == 0 → set_cooldown()
  │       ├── elapsed ≥ 60s → thermal_drain.cancel_jobs_for_provider()
  │       └── elapsed ≥ 90s → warn (drain stalled)
  │
  ├── Pass 1 — Scale-Out:
  │     for each scale_out_needed model:
  │       find candidate provider with free VRAM
  │       provisional_free[provider] -= needed_mb
  │       POST /api/pull to Ollama (async)
  │       set scale_out_holddown[provider] = now + holddown_ms
  │
  └── Pass 2 — Scale-In (idle eviction):
        for each loaded model on each provider:
          idle_secs >= threshold && not in scale_out_servers → evict
          POST {ollama}/api/generate keep_alive=0  (unloads model)
```

---

## Circuit Breaker

```
Per-provider circuit breaker — prevents routing to broken providers

States: Closed (normal) → Open (blocked) → Half-Open (probe)

record_failure(provider_id):
  consecutive_failures += 1
  threshold reached → Open  (TTL = backoff)

record_success(provider_id):
  consecutive_failures = 0
  if Half-Open → Closed

is_allowed(provider_id):
  Closed/Half-Open → true
  Open → false (skip at dispatch)
```

---

## Files

| File | Purpose |
|------|---------|
| `application/use_cases/inference/dispatcher.rs` | Queue loop, `select_provider()` |
| `application/use_cases/placement_planner.rs` | 5s planner — scale-out/in, thermal drain |
| `application/ports/outbound/concurrency_port.rs` | `VramPoolPort` trait |
| `infrastructure/outbound/vram_pool.rs` | VRAM pool implementation |
| `application/ports/outbound/circuit_breaker_port.rs` | Circuit breaker trait |
