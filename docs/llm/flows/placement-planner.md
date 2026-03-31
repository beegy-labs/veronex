# Model Auto-Scaling (Placement Planner)

> **Last Updated**: 2026-03-28

---

## Overview

5-second loop that makes automated model placement decisions:
scale-out, preload, evict, scale-in, and standby recovery.

---

## Tick Structure

```
planner_tick():
  clean expired hold-downs

  ── Pass 0: Read-only snapshot ──
  list all active Ollama providers
  scale_out_candidates = providers where:
    thermal ∉ {Soft, Hard, Cooldown}
    circuit_breaker.is_allowed
    available_vram_mb > 0

  collect all_models from VramPool loaded models
  parallel Valkey GET demand:{model} for each model
  compute eligible_capacity per model (exclude thermal/CB/pulling/blocked)
  scale_out_needed = models where demand > capacity × 0.80
  provisional_free = available VRAM per candidate (collision prevention)

  ── Hard Gate Watchdog ──
  ── Step ④: STANDBY recovery ──
  ── Step ①: Scale-Out ──
  ── Step ②: Preload ──
  ── Step ③: Evict ──
  ── Step ⑤: Scale-In ──
```

---

## Hard Gate Watchdog

```
for provider in Hard throttle:
  if active == 0 → set_cooldown()              // natural drain
  if hard_since >= 90s → ERROR log (drain stall)
  if hard_since >= 60s → force-cancel in-flight jobs
    // cancel drops VramPermit → active→0 → Cooldown
```

---

## Step ④: STANDBY Recovery

```
for standby provider (not in_transition):
  wake if:
    loaded model has demand > 0      OR
    best candidate for scale_out     OR
    ZSET queue_len > 0
  → set_standby(false), set_transition_until(now + 30s)
```

---

## Step ①: Scale-Out

```
for model in scale_out_needed:
  skip if preloading_count >= needed_servers
  best_server = max(provisional_free) among eligible candidates
    filter: not loaded, not preloading, not pulling, not excluded, not in_transition

  Valkey dedup:
    GET scaleout_decision:{model} → skip if exists
    SET scaleout_decision:{model} NX EX 30      // decision lock
    SET preload_lock:{model}:{server} NX EX 180 // preload lock

  update provisional_free -= model_weight
  set hold-down(server, now + 60s)
  spawn preload_model() task
    on complete: release locks, mark_model_loaded
```

---

## Step ②: Preload (Non-Scale-Out)

```
for model with demand > 0 (not in scale_out_needed):
  for provider (not thermal-gated, not standby):
    skip if loaded, preloading, pulling, excluded, no free VRAM
    SET preload_lock:{model}:{provider} NX EX 180
    spawn preload_model()
    break  // one preload per model per cycle
```

---

## Step ③: Evict

```
for provider, for loaded model:
  skip if demand > 0, active > 0, preloading, pulling
  if should_evict(idle_secs, is_standby):
    mark_model_unloaded()
```

---

## Step ⑤: Scale-In

```
guard: skip if only 1 provider OR queue_len > 0
for provider:
  skip if used in Scale-Out this cycle
  skip if in hold-down period
  skip if loaded models have demand, or active > 0, or preloading
  skip if already standby or in_transition
  → set_standby(true), set_transition_until(now + 30s)
```

---

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `PLANNER_INTERVAL` | 5s | Main loop interval |
| `SCALE_OUT_THRESHOLD` | 0.80 | Demand/capacity ratio trigger |
| `EVICT_IDLE_SECS` | 180s | Normal idle eviction threshold |
| `STANDBY_EVICT_IDLE_SECS` | 30s | Standby idle eviction threshold |
| `TRANSITION_GUARD_SECS` | 30s | Guard after standby state change |
| `SCALE_OUT_HOLDDOWN_SECS` | 60s | Prevent immediate scale-in after scale-out |
| `PRELOAD_LOCK_TTL` | 180s | Valkey preload lock TTL |
| `SCALEOUT_DECISION_TTL` | 30s | Valkey decision lock TTL |

---

## Files

| File | Role |
|------|------|
| `crates/veronex/src/application/use_cases/placement_planner.rs` | Planner loop + all steps |
| `crates/veronex/src/infrastructure/outbound/ollama/preloader.rs` | `preload_model()` |
| `crates/veronex/src/domain/constants.rs` | Key name helpers |
| `crates/veronex/src/bootstrap/background.rs` | Task spawn wiring |
