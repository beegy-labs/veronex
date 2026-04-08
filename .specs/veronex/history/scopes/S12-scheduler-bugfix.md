# S12 — Scheduler Critical Bugfix: Provider Model Mismatch

> **Status**: In Progress | **Created**: 2026-03-28 | **Severity**: P0

## Problem

Job requests for remote-only models (e.g., `qwen3-coder-next-128k`) get stuck in
`pending` indefinitely because:

1. **placement_planner preloads to wrong provider** — Scale-Out selects ANY healthy
   server regardless of whether it has the model in `ollama_models`. Local server
   gets `qwen3-coder-next-128k` preload → 404 (model not installed).

2. **VRAM pool not initialized for remote providers** — Remote Ollama providers have
   `available_vram_mb = 0` in VramPool because agent hasn't pushed capacity state.
   Dispatcher's `score_and_claim` filters `avail > 0` → remote excluded.

3. **Combined effect** — Dispatcher can't find any eligible provider → job stays
   pending → `queue_wait_exceeded` after 300s → user sees infinite loading.

## Root Cause Chain

```
Request: model=qwen3-coder-next-128k
  │
  ├─ dispatcher::filter_candidates()
  │   Stage 2: ollama_models → remote-only ✓
  │   Stage 4: preload_excluded → remote not excluded ✓
  │   ← candidates = [remote]
  │
  ├─ dispatcher::score_and_claim()
  │   vram.available_vram_mb(remote) = 0  ← agent hasn't pushed
  │   filter: avail > 0 → EXCLUDED
  │   ← no provider claimed
  │
  ├─ Meanwhile: placement_planner::Scale-Out
  │   scale_out_candidates = [local, remote]  ← no model filter!
  │   best_server = local (most free VRAM)
  │   preload(local, qwen3-coder-next-128k) → 404
  │   ← 3 failures → local preload_excluded
  │   ← keeps retrying local every 5s (infinite 404 loop)
  │
  └─ Result: job pending forever
```

## Fixes Applied

### Fix 1: placement_planner model filter (DONE)

```rust
// Before: any healthy server
let best_server = scale_out_candidates.iter()
    .filter(|p| { !loaded && !preloading && !excluded && free > 0 })

// After: only servers that have the model in ollama_models
let model_providers = ollama_model_repo.providers_for_model(model).await;
let best_server = scale_out_candidates.iter()
    .filter(|p| {
        model_providers.contains(&p.id)  // ← NEW
            && !loaded && !preloading && !excluded && free > 0
    })
```

File: `application/use_cases/placement_planner.rs`

### Fix 2: VRAM pool remote initialization (DONE)

Dispatcher fallback: when VramPool has no data (agent hasn't pushed yet),
use `provider.total_vram_mb` from DB as initial estimate.

```rust
let base = vram.available_vram_mb(b.id) as i64;
let base = if base == 0 && b.total_vram_mb > 0 { b.total_vram_mb } else { base };
```

File: `application/use_cases/inference/dispatcher.rs`

### Fix 3: Step ② Preload also lacks model filter (DONE)

Same bug in TWO places:
- Step ① (Scale-Out): selects best_server without model check → **Fixed**
- Step ② (Preload): iterates ALL ollama_providers without model check → **Fixed**

Both now filter by `ollama_model_repo.providers_for_model(model)` before preload.

### Structural Analysis

NOT a structural flaw. The planner was designed for single-server (all models on one server).
Multi-server with model distribution just needs the filter — no architecture change needed.

### Fix 3: Remote provider total_vram_mb = 0 (ROOT CAUSE)

01-setup registers remote provider without VRAM info → `total_vram_mb = 0` in DB.
Fix 2 fallback checks `b.total_vram_mb > 0` but DB value is 0 → no fallback.

**Root fix needed**: Provider registration must query Ollama `/api/ps` or `/api/show`
to get VRAM capacity, OR e2e setup must set `total_vram_mb` explicitly.

Interim: e2e setup sets `total_vram_mb = 131072` (128GB) for remote provider.

## Verification

- [ ] `qwen3-coder-next-128k` request → dispatched to remote-ollama (not local)
- [ ] No 404 preload errors in veronex logs
- [ ] 20-turn conversation test passes
- [ ] placement_planner logs show correct provider selection
