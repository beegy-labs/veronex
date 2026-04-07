# Multi-Instance Architecture

> **SSOT** for distributed coordination across Veronex replicas.

## Problem

With `replicas > 1` in Kubernetes (HPA/KEDA), five subsystems use instance-local state:

| Subsystem | Local state | Multi-instance issue |
|-----------|------------|---------------------|
| VRAM pool | DashMap + AtomicU32 | Each instance tracks independently → N× intended allocation |
| Job queue | BLPOP | At-most-once → crash after pop = lost job |
| Token streaming | DashMap | SSE client on instance B can't see tokens from instance A |
| Job status events | broadcast::Sender | Dashboard on instance B misses events from instance A |
| Cancellation | cancel_notify | Cancel on instance B can't interrupt job on instance A |

## Solution

All fixes use **Valkey** (already a dependency) as the distributed coordination layer. The hexagonal architecture is preserved: new adapters implement existing ports.

## Instance Identity

Each process generates `instance_id = Uuid::new_v4()` at startup (in `main.rs`). Stored as `Arc<str>`, passed to adapters that need it.

## Distributed VRAM Pool

**File**: `infrastructure/outbound/capacity/distributed_vram_pool.rs`

| Component | Purpose |
|-----------|---------|
| `DistributedVramPool` | Implements `VramPoolPort` using local `VramPool` + Valkey atomic Lua eval for VRAM reservation state |

**Strategy**: Local `VramPool` provides fast in-process reserve. Valkey Lua atomic evals (ACQUIRE/RELEASE/REAP) synchronize the shared reservation HASH and lease ZSET across instances. Each instance maintains its own `ProviderVramState` (total_mb, reserved_kv_mb, loaded_models, model_profiles).

**RAII**: `VramPermit` releases KV cache on drop (weight stays loaded). This is synchronized across instances via async Valkey publish.

**Lease management**: Each VRAM reservation creates a ZSET lease entry with `LEASE_DURATION_SECS=120` TTL. The lease member format is `instance_id|lease_id|kv_mb` (pipe-delimited — avoids collision with `:` in UUIDs and model names), embedding the KV allocation size for crash recovery.

**Lease lifecycle** (Lua scripts):
- `LUA_VRAM_ACQUIRE`: `HINCRBY reserved +kv` + `ZADD lease` (member includes kv_mb)
- `LUA_VRAM_RELEASE`: `HINCRBY reserved -kv` + `ZREM lease`
- `LUA_VRAM_REAP`: `ZRANGEBYSCORE expired` → for each: `ZREM` + `HINCRBY reserved -kv` (extracted from member)

The reaper deducts reserved HASH on lease expiry, preventing zombie reservations after instance crashes.

## Reliable Queue (ZSET — Phase 3)

**File**: `application/use_cases/inference/dispatcher.rs` — `queue_dispatcher_loop()`

| Before (LIST) | After (ZSET) |
|--------|-------|
| 3 LIST queues (paid/standard/test) | Single ZSET `veronex:queue:zset` with tier-scored entries |
| `BLPOP` / Lua LMOVE (at-most-once) | ZRANGE peek top-K → Rust scoring → Lua ZREM claim |
| Crash after pop = lost job | Processing list + `veronex:job:owner:{job_id}` tracks ownership |
| No crash recovery | Reaper + `recover_pending_jobs()` on startup |

**Model filter**: Four-stage filtering for Ollama jobs in the queue dispatcher:
1. `providers_for_model()` — active + provider_type match + tier check + **standby exclusion** (`!vram_pool.is_standby()`). Also filters providers that have the requested model installed (OllamaModelRepository).
2. `list_enabled()` — filters providers where the model is disabled in selection config (ProviderModelSelectionRepository).
3. Thermal + Circuit Breaker + Concurrency gates.
4. Preload exclusion — `is_preload_excluded()` filters providers where the model had 3 consecutive preload failures within 300s.

**Model stickiness**: Providers with the requested model already loaded in VRAM get a +100GB bonus in the availability sort, strongly favoring consecutive requests on the same provider over model switching.

**Lua scripts** (3 atomic): Enqueue (ZCARD+demand guard+ZADD+INCR+HSET×2), Claim (ZREM+ZADD active_lease+DECR+HDEL×2), Cancel (ZREM+DECR+HDEL×2). Dispatcher sleeps 500ms on empty ZSET.

**ACK**: On job completion/failure, `active_lease_remove()` → `ZREM QUEUE_ACTIVE {uuid}` + `DEL job:owner:{job_id}`.

**Fail fast on no candidates**: If zero eligible providers remain after model filtering, the job is ZSET-claimed then failed immediately.

**Queue full rejection** (capacity exceeded): When ZSET enqueue returns `Ok(false)` (`MAX_QUEUE_SIZE` = 10,000 or `MAX_QUEUE_PER_MODEL` = 2,000 exceeded), the job is:
1. Removed from in-memory DashMap + `cancel_notifiers`
2. Marked `Failed` in DB (`update_status(Failed)`) — orphan prevention
3. Returns `DomainError::QueueFull` to the caller
Without step 2, the DB job would remain in `Pending` state permanently since no queue worker will ever pick it up.

**No-provider (VRAM blocked)**: Job stays in ZSET (not claimed), dispatcher skips to next in window. Adaptive K: `min(ZSET_size/3, 100)`, floor 20.

### Tier Scoring + EMERGENCY_BONUS (Starvation Prevention)

```
ZSET score = now_ms - tier_bonus
  TIER_BONUS_PAID     = 300,000ms
  TIER_BONUS_STANDARD = 100,000ms
  TIER_EXPIRE_SECS    = 250s
```

- Within 250s: paid always processes before standard (200,000ms gap, non-reversible)
- After 250s: `promote_overdue` loop applies EMERGENCY_BONUS (300,000ms) → long-waiting requests overtake new paid requests

**Rust-side scoring** (computed per job in top-K window before Lua claim):
```
final_score = zset_score - locality_bonus - age_bonus
  locality_bonus = LOCALITY_BONUS_MS (20,000ms) if model is loaded on any provider, else 0
  age_bonus      = wait_ms × 0.25 × perf_factor   (perf_factor from ThermalPort.global_perf_factor())
```
Lower `final_score` = higher priority. Locality bonus gives a ~20s priority boost to jobs whose model is already warm.

**Note**: Two separate locality mechanisms:
1. **ZSET-level** (`LOCALITY_BONUS_MS = 20,000ms`): reduces final_score when model is loaded on *any* provider → job gets sooner dispatch
2. **Provider-selection** (`MODEL_LOCALITY_BONUS_MB = 100,000 MB ≈ 100GB`): sort bonus when selecting *which* provider to claim → favors provider that already has the model loaded

**EMERGENCY_BONUS is applied exclusively by `promote_overdue`** (single responsibility):
- Dispatcher's `final_score` uses raw ZSET score without EMERGENCY_BONUS deduction
- `promote_overdue` loop (30s) updates ZSET score via `ZADD XX`: `new_score = enqueue_at_ms - EMERGENCY_BONUS`
- This ensures overdue jobs enter top-K window and outrank new paid requests

### Demand Counter

Per-model queued job count, used by Placement Planner for scale-out decisions:

- **INCR**: Lua enqueue script (atomic with ZADD)
- **DECR**: Lua claim script + Lua cancel script (atomic with ZREM)
- Tracks **queued only** — processing/inflight jobs are not counted
- `demand_resync_loop` (60s): ZSET-based ground truth recount corrects any drift

### Cancellation Contract (Queued vs Processing)

Two completely separate paths:

```
Queued:     Lua ZREM + DECR demand + HDEL side hashes
            ZREM returns 0 → already dispatched → fall through to processing path

Processing: cancel_notify + active_lease_remove() (ZREM QUEUE_ACTIVE) + VramPermit drop
            Multi-instance: pub/sub cancel signal via veronex:pubsub:cancel:{job_id}
```

- Queued cancel is atomic (single Lua script) — no race with dispatcher claim
- After Lua cancel, `schedule_cleanup(jobs, uuid, JOB_CLEANUP_DELAY)` fires after 60s to remove the DashMap entry (sweeper only removes Pending entries; cancelled-but-never-dispatched jobs would leak without this)
- Processing cancel uses `cancel_notify` (tokio::Notify) for local jobs, pub/sub for remote

### Double-Execution Prevention

Three-layer defense against the same job running on two instances simultaneously:

1. **Lua CAS in reaper** (`LUA_REAP_OWNED_JOB`): atomically checks that `job:owner` still matches the expected dead instance AND that its heartbeat key is absent, before re-enqueueing. No TOCTOU race.
2. **Periodic owner refresh** in `run_job()`: every 60s refreshes `job:owner` TTL via `SET XX EX 300`. Prevents the reaper from seeing a stale owner key during long-running jobs.
3. **Ownership guard before final DB write** in `run_job()`: verifies `GET job:owner:{job_id}` still matches `instance_id` before persisting results. Aborts silently if ownership was transferred.

**Ownerless jobs**: Reaper uses `LUA_REAP_OWNERLESS_JOB` with `SET NX` to claim ownership before re-enqueue, preventing multiple reapers from racing.

## Queue Maintenance

**File**: `infrastructure/outbound/queue_maintenance.rs`

### promote_overdue_loop (30s)

Prevents tier-based starvation for long-waiting requests:

1. HSCAN `veronex:queue:enqueue_at` → iterate all `(job_id, enqueue_at_ms)` pairs
2. Filter: `now_ms - enqueue_at_ms > 250,000ms`
3. ZADD XX: `new_score = enqueue_at_ms - EMERGENCY_BONUS_MS` (only updates existing entries)
4. Subsequent ZRANGE naturally selects overdue jobs into top-K window

Side hash dependency: `veronex:queue:enqueue_at` stores original enqueue time (ZSET score alone is tier-adjusted, cannot recover original timestamp).

### demand_resync_loop (60s)

Corrects demand_counter drift from any source (Valkey restart, manual ops):

1. ZSCAN `veronex:queue:zset` → collect all job_ids (ZSET is single source of truth)
2. HMGET `veronex:queue:model` batch → map job_id → model
3. Aggregate per-model counts → SET `demand:{model}` count
4. **Stale GC**: remove `queue:enqueue_at` and `queue:model` entries for job_ids not in ZSET

Design: ZSET is the sole source of truth. Side hash (`queue:model`) alone is never used for counting — stale entries would cause over-counting.

### Startup Sequence (Crash Recovery)

```
① spawn health_checker_loop, sync_loop, session_grouping_loop
② use_case_impl.recover_pending_jobs()   ← re-enqueue DB-persisted Pending/Running jobs (explicit, non-fatal)
③ spawn queue_worker + job_sweeper
④ (Valkey only) spawn reaper, pub/sub subscribers, promote_overdue, demand_resync, placement_planner
```

Notes:
- `recover_pending_jobs()` (step ②) is the only explicit startup recovery call; it runs before queue_worker (step ③) to ensure orphaned jobs re-enter the ZSET before dispatching starts.
- `demand_resync_loop` (60s periodic) corrects demand counter drift on an ongoing basis — there is no one-time pre-spawn resync step.
- Reaper recovers Valkey processing-list orphans on its periodic tick, not a separate startup call.

→ See `distributed-ops.md` for pub/sub, TPM accounting, crash recovery, Valkey key registry, and wiring.
