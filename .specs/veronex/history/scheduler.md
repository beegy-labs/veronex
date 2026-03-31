# Intelligence Serving — Scheduler SDD

> **Status**: Implemented | **Last Updated**: 2026-03-14
> **Target**: AMD Ryzen AI Max+ 395 (APU, unified memory) · k8s-worker-ai-01 (RAM 124GB)

---

## Goals

**Veronex = Intelligence Gateway that unifies N Ollama servers**

Not a simple reverse proxy. Treats the entire server fleet as a single compute pool
and autonomously learns/decides optimal behavior.

```
Client
    ▼
[Veronex Gateway]  ← K8s multi-replica
    ├──► Ollama A  (k8s-worker-ai-01, 124GB)
    ├──► Ollama B  (future expansion)
    └──► Ollama N
```

**Level 1 — Maximize single-server utilization**

Push each Ollama server's model allocation and concurrent throughput to the limit.

- Each server has different VRAM/num_parallel (heterogeneous). AIMD learns independently per server×model pair.
- APU (AMD Ryzen AI Max+ 395): unified memory 124GB, bandwidth 256GB/s is the bottleneck.
  DRM VRAM reports 1GB incorrectly → replaced with node-exporter measured values.
- Keep models resident in VRAM for continuous processing without reloads. Lazy Eviction reclaims only when needed.
- Queue: composite scheduling with FIFO + Locality (prefer loaded models) + Age (promote long-waiting requests).

**Level 2 — Maximize cluster throughput + power optimization**

Maximize throughput (Goodput) across the entire server fleet while minimizing resource waste.

- Optimal resource utilization is priority #1: sufficient throughput → run minimum servers. Insufficient → Scale-Out. Excess → reclaim immediately.
- Power efficiency: reclaim idle server models via Lazy Eviction → Ollama memory release.
  Use idle state as effective low-power mode without OS shutdown.
  (no model resident ~5W vs resident ~70W)

**Level 3 — Hardware protection (Thermal)**

Push hardware to its limits, but protect servers via automatic throttling/blocking when entering temperature danger zones.

- perf_factor: temperature-proportional scheduling throttle → suppress model switching on overheated servers.
- Soft Gate (82°C): block new requests + prohibit Preload/Scale-Out.
- Hard Gate (90°C): full block → Cooldown 300s → Ramp-up → gradual Normal recovery.
- Thresholds: vendor-specific defaults + admin per-provider override.

**Additional design goals**

- Crash recovery: Lua atomic handoff prevents job loss. at-least-once + idempotent.
- Model Pull drain: automatic request blocking during pull → drain → pull → automatic reload.
- Multi-instance safety: Lua ZREM atomic claim. Prevents duplicate processing across instances.
- Single-server environment (current setup: k8s-worker-ai-01, 1 node): Scale-Out is no-op.
  Designed for multi-server expansion but operates safely on a single server.

---

## Measured Environment and Ollama Configuration

```
RAM 124GB · mem_available 120GB · DRM VRAM 1GB(BIOS) · GTT 128GB
Models: qwen3-coder-next 51GB · qwen3:30b 18GB · qwen3:8b 5GB · nomic-embed 274MB
```

**Recommended Ollama settings**:

```
OLLAMA_NUM_PARALLEL=<num_parallel>   ← same as provider registration value, AIMD upper bound
OLLAMA_KEEP_ALIVE=-1                 ← Veronex Lazy Eviction direct control
OLLAMA_MAX_LOADED_MODELS=0           ← VramPool controls via VRAM budget
OLLAMA_FLASH_ATTENTION=1
OLLAMA_KV_CACHE_TYPE=q8_0
```

---

## N×N×N Architecture (heterogeneous per server)

```
Gateway → N servers (each with different VRAM · num_parallel · supported models)
              └─ N models co-resident per server (within VRAM budget)
                      └─ N concurrent requests per model (up to num_parallel, AIMD-learned)
```

AIMD learns independently per `provider_id × model` pair. qwen3:8b on server A and qwen3:8b on server B are separate.

**VramPool structure**:

```
ProviderVramState (per server)
  ├── total_mb         ← mem_available_mb × (1 - safety_permil/1000)
  │                      node-exporter measured. safety_permil absorbs APU drift.
  ├── reserved_kv_mb   ← total KV sum (atomic CAS)
  ├── safety_permil    ← default 100 (10%). +50 on OOM detection, -10 gradual recovery.
  │                      max 500 (50%). OOM = try_reserve failure or Ollama 429.
  └── is_standby       ← AtomicBool (excluded from routing on Scale-In)

ModelState (per model)
  ├── weight_mb / kv_per_request_mb
  ├── is_loaded / is_preloading    ← AtomicBool (prevents duplicate Preload)
  ├── active_count / max_concurrent ← AIMD-learned value (≤ num_parallel)
  ├── sample_count                 ← AtomicU32 (reset to 0 on evict)
  ├── baseline_tps / baseline_p95_ms
  └── last_active_at               ← AtomicU64 (Lazy Eviction criterion)
```

**Cost rules**: loaded → KV only / not loaded → weight+KV / completed → KV released, weight stays resident.

**APU mem_available_mb drift handling**: drift from non-Ollama process memory consumption is
absorbed by `safety_permil` (default 10%). Re-measured via node-exporter in 30s sync loop.

---

## Core Mechanisms

### 1. AIMD — Autonomous per-server×model optimal concurrency learning

APU is memory bandwidth-bound (256 GB/s). AIMD automatically finds the saturation point.

| Phase | Condition | max_concurrent |
|-------|-----------|---------------|
| Cold Start | sample = 0 | `num_parallel` (start from upper bound → search downward) |
| AIMD | sample ≥ 3 | TPS ratio < 0.7 or p95 spike → max(1, current×3/4) immediate / ratio ≥ 0.9 → +1 |
| LLM correction | sample ≥ 10, after AIMD | **increase direction only**, max +2 post-processing |
| Restart | DB recovery | apply last learned value immediately |

> **Cold Start policy change** (differs from `capacity.md` cold_start=1):
> On APU, memory safety is independently handled by try_reserve + safety_permil, so
> there is no need to start conservatively at 1. Starting from num_parallel and letting AIMD
> quickly adjust downward is better for initial throughput.
> `docs/llm/inference/capacity.md` reflects this Cold Start policy.

Feedback: every 30s from ClickHouse `inference_events` (since `learning_epoch_started_at`, max 1h). Results saved to DB.
  Reason: if measurements from the previous environment mix in after evict→reload, Cold Start relearning is invalidated.
          Only data after learning_epoch_started_at should be aggregated for sample_count=0 reset to be meaningful.
  ClickHouse query timeout: if ClickHouse response exceeds 30s, the entire AIMD loop blocks.
    Query timeout = 10s. On timeout, skip AIMD update for that cycle (keep previous max_concurrent).
    Warn log after 3 consecutive timeouts. ClickHouse failure does not stop AIMD.
  Sudden external memory change: if mem_available_mb drops ≥15% from the previous value in the 30s sync loop,
    reset all ModelState sample_count=0 + learning_epoch_started_at=now_ms.
    Reason: external processes consuming 20GB+ memory invalidates previous baselines even without evict/pull.
    15% = 120GB × 15% = 18GB threshold (roughly one qwen3:8b model, meaningful environment change).
LLM correction is applied as post-processing within the same 30s loop after AIMD calculation. Order: AIMD → LLM correction.
LLM correction applies only in the increase direction; decrease decisions are AIMD's sole responsibility.
If AIMD decay (×3/4) occurred in the same loop, LLM upward correction is prohibited.
That is, the AIMD decay result is the final value for that cycle.

**baseline_tps / baseline_p95_ms update rules**:

"First stable measurement" definition: the point when `sample_count ≥ 3` (same as AIMD first activation criterion)

| Event | baseline_tps | baseline_p95_ms |
|-------|-------------|-----------------|
| Initialization (first loop with sample_count ≥ 3) | set to `current_tps` | set to `current_p95_ms` |
| Cycle where decay occurred | freeze (no update) | freeze (no update) |
| `ratio ≥ 0.9` for 3 consecutive cycles (stability confirmed) | update upward to `current_tps` | update downward to `current_p95_ms` (reflects latency improvement) |
| evict → `sample_count = 0` | reset to `0` | reset to `0` |

p95 spike decay condition: `current_p95_ms > baseline_p95_ms × 2`
  → if baseline_p95_ms = 0 (before initialization), p95 spike condition is disabled; only TPS ratio is used.

**APU specifics**: DRM 1GB → models (5~51GB) always exceed → VRAM gate bypassed.
Role separation: AIMD = throughput optimization / try_reserve + safety_permil = memory safety. Two paths operate independently.
On OOM (Ollama 429 or try_reserve failure):
  - safety_permil +50 → immediately reduce available memory ceiling
  - Immediately apply max(1, current×3/4) to the model×provider max_concurrent (AIMD decay rule)
    **Floor = 1 is mandatory**: repeated u32 integer decay can reach 4→3→2→1→0.
    max_concurrent=0 means no requests accepted → no samples collected → AIMD increase condition unmet → unrecoverable deadlock.
  - Normal AIMD learning resumes in subsequent 30s loops
  **Asymmetric OOM recovery is intentional conservative design**:
    safety_permil recovery: -10/30s → 150s to recover from +50. max_concurrent recovery: +1/30s.
    Speed asymmetry between the two paths creates a ~150s underutilization window. This is intentional.
    OOM can cause full service disruption, so safety-first over fast recovery is rational.

### 2. Scheduling — FIFO + Locality + Age

**Queue**: Valkey ZSET. enqueue score = `now_ms - tier_bonus` (fixed). Adjusted at dispatch.

**Max queue size**: `MAX_QUEUE_SIZE = 10_000`. Reject enqueue on ZCARD overflow → 429 Too Many Requests.
  Reason: prevent unbounded ZSET growth. Valkey memory protection + K=100 scoring cost ceiling.
  **Atomicity required**: non-atomic ZCARD check followed by ZADD allows another writer to interleave → overshoot past 10,000.
  **Per-model/tier monopoly prevention**: a global cap alone lets a hot model or paid tier monopolize all 10,000 slots,
    fully blocking new requests for other models/tiers.
    Per-model cap: `MAX_QUEUE_PER_MODEL = 2_000` (demand_counter already provides per-model counts).
    In enqueue Lua, if demand:{model} ≥ MAX_QUEUE_PER_MODEL, reject only that model with 429.
    (Global ZCARD check AND per-model demand check — reject if either exceeds)
  Implementation: single Lua script — atomic ZCARD check + ZADD + INCR demand.
  ```lua
  -- Lua enqueue: atomic ZCARD check + ZADD + demand INCR + side hash write
  -- KEYS[1]=queue:zset  KEYS[2]=demand:{model}  KEYS[3]=queue:enqueue_at  KEYS[4]=queue:model
  -- ARGV[1]=job_id  ARGV[2]=score  ARGV[3]=max_size  ARGV[4]=now_ms  ARGV[5]=model
  if redis.call('ZCARD', KEYS[1]) >= tonumber(ARGV[3]) then return 0 end  -- 429
  redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
  redis.call('INCR', KEYS[2])
  redis.call('HSET', KEYS[3], ARGV[1], ARGV[4])  -- store enqueue_at (for promote_overdue lookup)
  redis.call('HSET', KEYS[4], ARGV[1], ARGV[5])  -- store model (for demand_resync lookup)
  return 1
  ```
  dispatch Lua handoff and queued cancel Lua also include `HDEL veronex:queue:enqueue_at {job_id}`.

**job→model mapping side hash** (for demand_resync):
  Added to enqueue Lua script: `HSET veronex:queue:model {job_id} {model}`
  Added to dispatch/cancel Lua: `HDEL veronex:queue:model {job_id}`
  Reason: demand_resync_loop (60s) needs to derive model from ZSET member (job_id),
          but ZSET members contain no model info. Resolved via side hash.
          promote_overdue_loop can also look up model from this hash.

**Tier priority (absolute within 250s wait, fair competition after)**:
```
TIER_BONUS_PAID     = 300,000ms
TIER_BONUS_STANDARD = 100,000ms
TIER_BONUS_TEST     = 0ms
TIER_EXPIRE_SECS    = 250s   ← tier_bonus invalidated after this time
```
Within 250s wait: paid is always processed before standard (200,000ms gap, no reversal possible).
Beyond 250s wait: tier_bonus invalidated + EMERGENCY_BONUS applied → **long-waiting requests take priority over new paid**.
  Precision guarantee: due to promote_overdue loop period of 30s, actual promotion occurs somewhere in the 250s~280s range.
  The exact contract is "guaranteed to be ahead within 280s at most", not "immediately ahead after 250s".

```
EMERGENCY_BONUS = TIER_BONUS_PAID = 300,000ms

final_score = zset_score                                ← use raw ZSET score as-is
            - locality_bonus  (model loaded: 20,000ms / not loaded: 0)
            - age_bonus       (wait_ms × 0.25 × perf_factor(temp_c))
Lower score = higher priority.
```

**EMERGENCY_BONUS application path — promote_overdue sole responsibility**:
  dispatch's final_score calculation does not apply EMERGENCY_BONUS.
  EMERGENCY_BONUS is applied only via `promote_overdue` loop (30s) which directly updates ZSET score with ZADD XX.
  dispatch trusts the updated raw score.
  Reason: applying EMERGENCY_BONUS in dispatch too would cause double deduction with promote_overdue.
          Single responsibility ensures correction logic exists in only one place.

Why simply removing tier_bonus is insufficient: with continuous paid inflow, a standard request past 250s
  has original score = T_old - 100,000, and new paid score = T_new - 300,000.
  If T_new = T_old + 250,000 then new paid score = T_old - 50,000.
  ZSET prioritizes lower scores: standard(T_old - 100,000) < paid(T_old - 50,000) → standard wins.
  // ↑ With naive tier_bonus removal ("overdue score = T_old"):
  //   overdue standard score = T_old, new paid score = T_old - 50,000
  //   T_old - 50,000 < T_old → paid wins — starvation is not resolved.
  // Therefore, EMERGENCY_BONUS must be added rather than simply removing tier_bonus:
  //   overdue standard score = T_old - 300,000 → lower than paid(T_old - 50,000) → standard wins.
When promote_overdue updates overdue standard's score to enqueue_at - EMERGENCY_BONUS:
  overdue standard score = T_old - 300,000 → definitively lower than new paid(T_old - 50,000) → standard wins.

Verification example (T_old = standard enqueue time, T_now = T_old + 251,000):
  After promote_overdue, standard score: T_old - 300,000
  New paid (wait=1s):      final = (T_now - 300,000) - locality - age ≈ T_now - 300,250
  standard (wait=251s):    final = (T_old - 300,000) - locality - age
                           = (T_now - 251,000 - 300,000) - 62,750  = T_now - 613,750  -> standard wins
  paid (wait=251s):        after promote_overdue score = T_old - 300,000 → same score as standard  -> fair race

**Intent**: requests waiting 250s+ always take priority over new requests regardless of tier.
  Standard starvation does not occur even under continuous paid inflow.

**EMERGENCY_BONUS top-K entry guarantee**:
  Since dispatch's final_score uses raw ZSET score, overdue jobs' ZSET scores must be
  directly updated to pull them into the top-K window.
  Under heavy load, if paid jobs fill K=20~100 slots, overdue standard jobs
  have high scores and cannot enter top-K at all.

  Solution: `promote_overdue` pass — separate 30s loop with **enqueue_at-based full cursor scan**:
    1. HSCAN veronex:queue:enqueue_at CURSOR COUNT 200 → iterate all (job_id, enqueue_at_ms)
    2. Filter only jobs where wait_ms = now_ms - enqueue_at_ms > 250,000
    3. Update the job's ZSET score via ZADD XX:
         new_score = enqueue_at_ms - EMERGENCY_BONUS
    4. Subsequent ZRANGE naturally selects overdue jobs into top-K based on raw score.
  Guarantee scope: both "standard priority after top-K entry" + "top-K entry guaranteed" hold.

  **Why scanning top K*3 ZSET scores is insufficient**:
    ZRANGEBYSCORE LIMIT 0 {K*3} only sees the top entries with low scores (=high priority).
    With thousands of paid jobs queued, overdue standard/test jobs have high scores buried beyond K*3
    and may never be promoted. HSCAN over the enqueue_at side hash iterates all jobs regardless
    of score order, detecting every overdue job.
    MAX_QUEUE_SIZE=10,000 × HSCAN COUNT 200 = max 50 iterations. Light enough for a 30s cycle.

  **Tier reverse-lookup problem solved**:
  ZSET score = `now_ms - tier_bonus`, so the original enqueue_at_ms cannot be derived from score alone.
  Solution: store `HSET veronex:queue:enqueue_at {job_id} {now_ms}` in a side hash at enqueue.
    HSET added to enqueue Lua script (atomic execution).
    promote_overdue: HSCAN veronex:queue:enqueue_at → obtain enqueue_at_ms.
    On dispatch/cancel: `HDEL veronex:queue:enqueue_at {job_id}` cleanup.
    new_score = enqueue_at_ms - EMERGENCY_BONUS  (no tier_bonus reverse-lookup needed, uses enqueue_at directly)

**Starvation prevention**: age_bonus ≥ locality_bonus reversal point → ≤75°C: 80s / 82°C: 114s.
Reversal guaranteed within max_queue_wait (300s). Unloaded models also reverse within ~2min → forces model switch.
SLA policy: no distinction between interactive/batch. No per-model differentiation.

**perf_factor × age_bonus design intent**: higher temperatures reduce age_bonus, delaying model switches.
This is intentional. Model reload (VRAM reallocation) on an overheated server creates additional
compute load, so for thermal protection it is better to keep serving the currently loaded model longer.

**Multi-instance safety**: ZRANGE K=20 peek (read-only) → Rust scoring → Lua ZREM atomic.
ZREM return 0 = another instance claimed → immediate retry.
K=20 window fairness: age_bonus is applied during Rust scoring after top-K candidate selection,
  so it has no effect on pulling jobs from outside K into the top of the ZSET.
  Fairness for jobs outside K is guaranteed only by the promote_overdue loop (30s) directly updating ZSET scores.
  The claim that cumulative age_bonus guarantees K entry is incorrect — promote_overdue is the sole responsible party.
Queue congestion mitigation: when ZSET size exceeds K×3 (60), dynamically expand K to min(ZSET_size/3, 100).
Checked via ZCARD on each dispatcher loop. Upper bound of 100 limits scoring cost.

**perf_factor(temp_c)**: ≤75°C → 1.0 / 82°C → 0.70 / ≥90°C → 0.0 (linear interpolation, thermal.rs).

**demand_counter**: `veronex:demand:{model}` (Valkey). Semantics = **ZSET queue length (queued only)**.
- INCR: when job enters ZSET (enqueue)
- DECR: atomic within Lua handoff script at dispatch (ZREM + LPUSH + DECR single script)
- DECR: cancel/timeout path — queued cancel Lua script (ZREM + DECR) atomic (see §7)
- In-flight (processing) jobs are not counted
- resync: every 60s, recount from ZSET members and overwrite → automatic INCR/DECR drift correction
          (ZSET is single source of truth: ZSCAN → HMGET queue:model → aggregate. Side hash stale entries auto-excluded)

**Atomicity scope specification**:
- enqueue: `ZCARD` + `ZADD queue:zset` + `INCR demand:{model}` — **single Lua script (atomic)**
  Reason: non-atomic ZCARD → ZADD allows another writer to interleave → MAX_QUEUE_SIZE overshoot.
          Lua bundling guarantees hard cap (return 0 = queue full → 429).
- dispatch: `ZREM + LPUSH + DECR` — **single Lua script (atomic)**
- cancel/timeout: `ZREM + DECR` — **single Lua script (atomic)**

**Drift safety rationale**:
- Standalone DECR failure impossible: DECR always runs alongside ZREM within a Lua script
- Enqueue drift eliminated: ZCARD + ZADD + INCR in single Lua script, so
  "crash after ZADD succeeds but before INCR" scenario is structurally prevented
- Resync exists for: defense against external exceptions (Valkey restart, operator manual ZSET manipulation, etc.).
  60s resync recalculates demand_counter from ZSCAN (ZSET as single source of truth), auto-correcting any exceptional drift.
  Standalone HSCAN of side hash (queue:model) prohibited — causes over-recovery from stale entries.
- Conclusion: no enqueue drift path. dispatch/cancel are Lua-atomic. 60s resync is the final safety net.
  Permanent drift is impossible.

### 3. Thermal Protection — Request blocking and recovery

**Threshold basis**: AMD Ryzen AI 395+ APU official junction temp limit (105°C) with
operational safety margin. 75°C (normal) / 82°C (warning) / 90°C (critical) 3 zones.
Admin can override per provider. Vendor defaults:
- AMD APU (Ryzen AI Max+ 395): normal_below=75 / soft_at=82 / hard_at=90 / cooldown_secs=300
- NVIDIA GPU: normal_below=80 / soft_at=88 / hard_at=93 / cooldown_secs=300
- unknown: AMD APU defaults applied

**cooldown_secs=300 rationale**: GPU/APU thermal throttling requires minutes for hardware clock
recovery + sensor stabilization even after software stops the load. Resuming at 60s has high probability
of "resume load → immediate throttling" infinite loop. 300s (5min) is the minimum sufficient for APU cooldown. Admin override available.

health_checker 30s loop → node-exporter scrape → `thermal.update(temp_c)` → state update.
dispatcher reads current thermal state on `score_and_claim()` call (atomic load).

**Soft Gate (≥ soft_at, 82°C)**:
```
New requests: blocked (503)
In-flight requests: allowed to complete
Release condition (Hysteresis): return to Normal when temp < normal_below AND provider_total_active == 0
  // normal_below: auto-set per provider (gpu_vendor=amd → CPU thermal baseline 75°C, nvidia → GPU thermal baseline)
  // SDD initial 80°C → 75°C change reason: AMD APU CPU thermal lags behind GPU thermal,
  //   so wider hysteresis margin (soft_at 82 - normal_below 75 = 7°C) is more effective at preventing oscillation.
  // provider_total_active = Σ active_count(model, provider) for all loaded models on this provider
  // active_count is per ModelState (model+provider pair), so provider-wide sum is the release condition.
  // Not model-wide active_count alone — all in-flight requests across all models on that provider must finish.
Intent: prevent oscillation where gate opens/closes per single request at the 82°C boundary.
        provider_total_active == 0 alone does not release — temperature must drop below normal_below to resume.
Release check frequency: only in health_checker 30s loop. No event-driven immediate release.
  (even when provider_total_active==0 event fires, temperature recheck waits for next 30s loop — conservative intent)
Long-stream stuck prevention: SSE_HARD_TIMEOUT_SECS = 600 (constant).
  Used as forced termination threshold in dispatcher.rs and runner.rs.
  All in-flight streams terminate within 600s after Soft Gate entry — indefinite stuck impossible.
  // This guarantee is self-contained in scheduler.md. Holds without referencing distributed.md.
  // Changing SSE_HARD_TIMEOUT_SECS breaks this guarantee, so both documents must stay in sync.
```

**Hard Gate (≥ hard_at, 90°C)**:
```
New requests: all blocked (503)
In-flight requests: allowed to complete (max 60s drain cap)
  Reason: unbounded drain cannot guarantee actual cooldown time.
          If a long SSE runs 200s more, only 100s of the 300s cooldown is actual cooling → 300s rationale invalidated.
// Terminology — to avoid confusion:
//   forced_drain_timeout = 60s  : max wait time after Hard entry before force-terminating in-flight.
//                                  Unrelated to cooldown period (300s). If drain finishes quickly, it may complete in 0s.
//   cooldown_secs = 300s        : Cooldown state duration. Actual hardware cooling time.
//                                  Changed from legacy 7,200s → 300s (rationale: L322-324).
Cooldown timer start — single definition:
  timer_start_at = first_time_provider_total_active_reaches_0
  // = point after Hard entry when provider_total_active (sum of all model active_counts) reaches 0.
  // VramPermit drop (step 5 completion) decrements active_count, so timer starts after actual hardware load ends.
  // Due to forced drain (60s cap), this is within max 60s + cancel→VramPermit drop delay (seconds) after Hard entry.
  // "min(hard_entered_at+60s, active==0)" approach starts timer before VramPermit drop (before step 5),
  // counting down cooldown while hardware is still under load — prohibited.
  watchdog: if active>0 after 90s (=60s drain + 30s buffer) since Hard entry, log error and
            force-set timer_start_at = hard_entered_at + 90s (prevent blocking).
  Reason: cannot wait indefinitely for cancel() completion, so 90s watchdog is the final guarantee.
Additional dispatch: none after Hard entry. No follow-up dispatch for requests completing during drain.

[Hard Gate forced drain cancel — forced termination contract when 60s cap exceeded]
  Trigger: 60s elapsed after Hard entry && active_count > 0 (drain incomplete)
  Processing (same as §7 processing cancel path):
    1. Send cancel signal to each in-flight job's Job Runner
    2. Send SSE error event (if stream is still open):
         data: {"error":{"type":"thermal_hard_gate","message":"server temperature critical"}}\n\n
    3. LREM processing {job_id}
    4. DB job status = "failed", failure_reason = "thermal_hard_gate"
    5. VramPermit drop → KV released, active_count decremented
  Order guaranteed: 1→2→3→4→5.
  // Cooldown timer start: after step 5 completion (VramPermit drop → active_count decrement),
  // when provider_total_active == 0. 90s watchdog is the final guarantee (see single definition above).
  // Step 5 must complete after forced drain cancel fires (60s) before active==0 is confirmed.
  // Starting timer at "cancel fire time (60s)" means hardware is still computing — prohibited.
  Duplicate terminal prevention: same mechanism as CancelOnDrop — skip if runner already wrote DB status after cancel().
  VramPermit drop timing contract:
    cancel() signal sent → Job Runner SSE loop breaks → steps 2~4 complete → step 5 VramPermit drop.
    Ollama begins KV slot release via RST_STREAM (HTTP/2) or connection close event.
    Veronex's VramPermit drop does not confirm Ollama's internal release completion.
    (soft reservation via try_reserve, so Ollama 429 recurrence is naturally corrected via AIMD/OOM path)
```

**Thermal state machine**:
```
Normal ──[≥soft_at]──► Soft ──[≥hard_at]──► Hard ──[active==0 OR 60s drain]──► Cooldown
  ▲                     │                     ▲                                       │
  └─[<normal_below/hyst]─┘                     │               cooldown_secs elapsed   │
                                               │                    → enter RampUp ────┘
RampUp (separate state):
  - **Accepts new requests** (no blocking). Only max_concurrent=1 cap applied.
    Different from Soft (503 blocking). Gradual serving resumption stage before Normal return.
  - max_concurrent = 1 forced
  - Temperature check every 30s with branching:
      temp < soft_at              → resume AIMD +1 (continue RampUp)
      soft_at ≤ temp < hard_at   → transition to Soft (resume request blocking)
      temp ≥ hard_at             → transition to Hard (immediate block + re-enter Cooldown)
  - Provider-wide recovery condition → full Normal return:
      temp < normal_below
      AND Σ max_concurrent(model, provider) for all loaded models ≥ provider_pre_hard_total
    // provider_pre_hard_total: snapshot of provider_total_committed_parallel just before Hard entry (stored in ProviderVramState).
    // Provider-wide sum criterion, not per-model pre_hard_max_concurrent.
    // Reason: thermal is provider-wide temperature, so recovery condition should also be provider-wide for consistency.
    //   Even if 1 model reaches pre_hard, if other models are still at 1 then actual load hasn't reached pre_hard level.
    // RampUp → Hard re-transition: keep existing provider_pre_hard_total (do not redefine).
    //   On Hard re-entry during RampUp, RampUp's reduced state must not be overwritten.
    // pre_hard_max_concurrent (per-model): kept for RampUp progress display. Auxiliary to provider-wide condition.

Temperature re-rise during Cooldown:
  temp ≥ hard_at → Cooldown timer reset (restart cooldown_secs)
  Cooldown exit condition: cooldown_secs elapsed AND temp < soft_at
    (if temperature is still ≥ soft_at at expiry, remain in Cooldown)
  **Cooldown max wait cap**: cooldown_secs × 3 from initial Cooldown entry time (default 900s = 15min)
    Absolute cap independent of timer resets. cooldown_entered_at recorded once at Hard→Cooldown transition, not updated on reset.
    On cap reached: check temperature before transition, then branch
      temp ≥ hard_at   → re-enter Hard (reset Cooldown timer, do not enter RampUp)
      soft_at ≤ temp < hard_at → transition to Soft (resume request blocking)
      temp < soft_at   → transition to RampUp
    Reason: if external heat source (other workloads) holds temp at 82~89°C, Cooldown extends indefinitely.
            Unconditional RampUp entry on cap would create a safety gap where new requests are
            accepted for up to 30s at ≥hard_at. Temperature check branching prevents this.
            Admin alert log recorded.
```

**Transition completeness guarantee** (all paths defined):
```
Normal    → Soft      : temp ≥ soft_at
Soft      → Hard      : temp ≥ hard_at
Soft      → Normal    : temp < normal_below AND provider_total_active == 0 (hysteresis)
Hard      → Cooldown  : provider_total_active == 0 (max 60s forced drain after Hard entry, 90s watchdog final guarantee)
Cooldown  → Cooldown  : temp ≥ hard_at (timer reset) or still temp ≥ soft_at (wait)
Cooldown  → RampUp    : cooldown_secs elapsed AND temp < soft_at
Cooldown  → Hard      : cooldown_secs × 3 elapsed AND temp ≥ hard_at (Hard re-entry)
Cooldown  → Soft      : cooldown_secs × 3 elapsed AND soft_at ≤ temp < hard_at
Cooldown  → RampUp    : cooldown_secs × 3 elapsed AND temp < soft_at
RampUp    → RampUp    : temp < soft_at, AIMD learning in progress
RampUp    → Soft      : soft_at ≤ temp < hard_at
RampUp    → Hard      : temp ≥ hard_at
RampUp    → Normal    : AIMD current ≥ pre_hard_max_concurrent AND temp < normal_below
```

**Circuit Breaker vs Thermal Gate priority**:
```
Circuit Breaker: provider unresponsive/timeout → block entire provider
Thermal Gate:    temperature exceeded → block new requests on that provider
Simultaneous activation: Circuit Breaker takes priority (stronger block). Re-evaluate Thermal state after CB release.
Release order: Thermal cooldown complete → CB half-open → CB release on successful probe request.
```

**All-provider unavailable behavior — two paths, single definition**:
```
// Path A (pre-handoff): filter_candidates()=0 detected at ZRANGE peek stage
//   → skip entire dispatch cycle. No ZREM. All jobs preserved in queue.
//   → wait until next loop (QUEUE_POLL_INTERVAL).
//   Reason: prevent HOL blocking. Consuming queue front would cause repeated consume-fail in same state next cycle.
//   Client handling: provider recovers within max_queue_wait (300s) → normal dispatch.
//                    No recovery → max_queue_wait exceeded → queued cancel path (§7) → SSE error event.

// Path B (post-handoff exception): provider state change within score_and_claim() → eligible=0
//   → job is already in processing state. Execute processing sequence below immediately.
//   Reason: cannot return to queue after ZREM completed. Processing-state jobs must be terminated immediately.

Dispatch flow:
  1. ZRANGE peek → collect top-K candidates
  2. filter_candidates() call — if 0 eligible providers:
       Path A: skip entire cycle (no ZREM). Retry after QUEUE_POLL_INTERVAL.
  3. If eligible providers exist → Rust window scoring → select best job → Lua handoff
  4. score_and_claim() — if eligible=0 detected at this stage, execute Path B

Path B processing sequence (only failure_reason differs per case):
  Case A: all Hard gate / Circuit Breaker / is_pulling → failure_reason="no_eligible_provider"
  Case B: all Soft gate (or mixed) → failure_reason="all_providers_soft_gated"

  1. LREM processing {job_id}   ← cleanup processing list (required since ZREM already completed)
  2. DB status="failed", failure_reason recorded
  3. Client response (branch based on whether HTTP response has started):
       [200 OK not yet sent] → return 503 + Retry-After header
       [SSE heartbeat already sent (200 OK + SSE headers)] → send SSE error event then close stream:
         data: {"error":{"type":"no_eligible_provider","message":"...","retry_after_secs":N}}\n\n
       Reason: status code cannot be changed after HTTP response has begun.

Retry-After calculation rules (Path B only, single implementation definition):
  Hard gate: max(0, cooldown_secs - elapsed_cooldown_secs). If not yet in cooldown, use cooldown_secs.
  Circuit Breaker: CB half-open wait time (next_attempt_at - now, provided by CB implementation).
  is_pulling: use max_pull_secs default (remaining time unpredictable). Default 300s.
  Soft gate: based on health_checker 30s loop period. Default 30s.
  Multiple states mixed: minimum of above values.
  Unknown: default 60s.
```

### 4. 3-State Model Lifecycle

```
IDLE ──[demand>0 + Preloader]──► COLD START ──[load complete]──► STEADY STATE
 ▲                                                                 │
 └──────────────── Lazy Eviction ─────────────────────────────────┘
      ① Another model needs VRAM (APU: ②③ alone trigger eviction)
      ② active_requests == 0
      ③ idle ≥ 180s  (is_standby=true server: shortened to idle ≥ 30s — power optimization priority)
      → on evict: sample_count = 0, learning_epoch_started_at = now_ms
                  (Cold Start restarts on reload + ClickHouse aggregation starts from new epoch)
```

Preloader: POST `/api/generate` `num_predict=0`. is_preloading flag prevents duplicates.

**Preload failure handling**:
```
120s timeout or error → is_preloading=false → retry on next 5s loop
Client timeout defense:
  - Queued requests receive SSE heartbeat ("data: \n\n") every 30s during wait (keep connection alive)
  - max_queue_wait = 300s. On exceed → job → failed, client response:
      Before SSE heartbeat sent: return 503
      After SSE heartbeat sent: send SSE error event then close stream
        data: {"error":{"type":"timeout","message":"queue wait exceeded"}}\n\n
  - After 3 consecutive preload failures: exclude only that **model+provider pair** for 300s
      If other healthy providers exist → continue routing to those providers
      Only when all providers are excluded → return 503 for that model's requests
      Reason: "503 for all requests of that model" blocks healthy providers in multi-provider setups, breaking availability.
  - Long-term recovery after 3 failures: reset preload_fail_count=0 after 300s → auto-retry resumes
    During 300s the model+provider pair is excluded from dispatcher filter_candidates() and Planner ①② loops
```

### 5. Model Pull — Request Drain

Automatically blocks requests during Ollama model add/replace and resumes after completion.

**Permissions**: all Pull/Drain APIs require JWT admin or higher. Not accessible via regular API keys.
  Reason: 50GB+ model pull occupies server for up to 4 hours — allowing regular user triggers enables DoS.

```
POST   /v1/ollama/models/pull {model, provider_id}   ← RequireAdmin
// DELETE /v1/ollama/models/pull/{provider_id}/{model} — simplified: not implemented.
//   Auto-released via max_pull_secs (4h) timeout, so manual cancel unnecessary.
//   Implement when needed. (See G13)

POST /v1/ollama/models/pull {model, provider_id}
  → set is_pulling=true
  → new requests: that model+provider → 503 (pull in progress), held until is_pulling cleared

  [Stage 1 — Drain] wait until active_count==0
    If drain timeout exceeds 60s: force proceed (full §7 processing cancel path)
      → In-flight SSE handling (503 impossible since 200 OK + SSE headers already sent):
          1. Send cancel signal to Job Runner
          2. Send SSE error event then close stream:
               data: {"error":{"type":"service_update","message":"model pull in progress"}}\n\n
          3. LREM processing {job_id}     ← omission leaves zombie jobs
          4. DB job status = "failed", failure_reason = "drain_forced"
          5. VramPermit drop → KV released, active_count decremented
          Order guaranteed: 1→2→3→4→5 (same as §7)
      → Force pull proceeds; model+provider remains is_pulling=true since Ollama is replacing the model

  [Stage 2 — Pull] execute ollama pull
    Tracking fields:
      started_at:    pull start time
      max_pull_secs: default 14400 (4h), admin override available
      // heartbeat_at — simplified: not implemented. Single max_pull_secs timeout is sufficient.
      //   Timeout determined by started_at + max_pull_secs without parsing Ollama pull progress stream. (See G14)
    Timeout determination: (now - started_at) > max_pull_secs
    On timeout: force-clear is_pulling=false, error log, resume Planner
    Reason: prevent permanent serving paralysis of a specific model due to pull hang.

  [Stage 3 — Resume] on completion:
    is_pulling=false, is_loaded=false
    sample_count=0, learning_epoch_started_at=now_ms
    baseline_tps=0, baseline_p95_ms=0
    preload_fail_count=0, preload_failed_at=0   ← explicit initialization (prevent stale exclusion)
    Reason: pull is a model weight replacement event — larger environment change than evict.
            Without epoch update, past 1h data mixes into new relearning → Cold Start invalidated.
            Without baseline reset, AIMD converges incorrectly using old model baselines.
    → Placement Planner auto-reloads via Preloader on next 5s loop (Cold Start restarts)
```

**Manual block**: `PATCH /v1/providers/{id}/selected-models/{model}` ← RequireAdmin → `is_enabled=false`
to block directly before pull. Re-enable with `is_enabled=true` after completion.

### 6. Hard Gate Forced Drain Cancel Contract

The Hard Gate 60s forced drain cancel processing contract is defined in §3 Hard Gate (see §3).
Follows the same processing cancel path as §7. failure_reason = "thermal_hard_gate".

### 7. Cancellation and Timeout Contract

**Background**: current code (`cancel_guard.rs`, `runner.rs`, `use_case.rs`) implements the processing-state
job cancel path (CancelOnDrop → cancel() → DB cancelled + publish).
However, the ZREM + demand DECR path for queued-state (in Valkey ZSET) jobs does not exist in code.
This SDD explicitly defines it.

**Two-state cancel path separation**:

```
[queued cancel]  job is in ZSET (not yet dispatched)
  Common processing (trigger-agnostic):
    1. Lua atomic script: ZREM queue:zset {job_id} + DECR demand:{model}
                          + HDEL queue:model {job_id} + HDEL queue:enqueue_at {job_id}
       (ZREM return 0 = already dispatched → switch to processing cancel)
    2. DB job status / client response branches per trigger:

  [client disconnect]   DB status = "cancelled"
                        No SSE response sent (connection already closed)

  [max_queue_wait exceeded] DB status = "failed", failure_reason = "queue_wait_exceeded"
                        // G15: queue_maintenance.rs::run_queue_wait_cancel_loop() implemented
                        Error event on SSE heartbeat connection then close:
                          data: {"error":{"type":"timeout","message":"queue wait exceeded"}}\n\n

  [manual cancel API]   DB status = "cancelled"
                        Cancel event on SSE heartbeat connection then close:
                          data: {"error":{"type":"cancelled","message":"request cancelled"}}\n\n

[processing cancel]  job is in processing list (runner executing)
  Triggers: client disconnect (CancelOnDrop) / runner internal error / pull drain forced termination
  Processing:
    1. cancel() call → runner SSE loop breaks
    2. SSE error event sent (if stream still open): drain_forced or client_disconnect
    3. LREM processing {job_id}
    4. DB job status:
         client disconnect → "cancelled"
         pull drain forced / runner error / timeout → "failed" (failure_reason varies)
    5. VramPermit drop → KV released

[timeout cancel]  max_queue_wait (300s) exceeded
  If queued → queued cancel path
  If processing → Ollama response timeout → runner error → processing cancel path
```

**Unified Cancellation Contract**:

```
Cancel reason          | State      | Valkey handling           | DB status | Client
-----------------------|------------|--------------------------|-----------|------------------
client disconnect      | queued     | ZREM + DECR (Lua atomic) | cancelled | SSE heartbeat closed
client disconnect      | processing | cancel() + LREM          | cancelled | connection closed
max_queue_wait         | queued     | ZREM + DECR (Lua atomic) | failed    | SSE error event
pull drain forced      | processing | cancel() + LREM          | failed    | SSE error event
Ollama timeout         | processing | cancel() + LREM          | failed    | SSE error event
thermal hard forced    | processing | cancel() + LREM          | failed    | SSE error event
manual cancel API      | queued     | ZREM + DECR (Lua atomic) | cancelled | SSE error event
manual cancel API      | processing | cancel() + LREM          | cancelled | SSE error event
no_eligible_provider   | processing | LREM                     | failed    | 503 or SSE error (see §3)
```

**LREM guarantee principle**: all Job Runner exit paths (normal completion, error, timeout included)
must perform `LREM processing {job_id}`.
LREM is part of final cleanup regardless of whether cancel() was called.
Failure to perform leaves zombie jobs in the processing list → contaminates startup recovery re-execution.

**Multi-instance safety**: if queued cancel Lua script's ZREM returns 0,
  another instance already dispatched → switch to processing cancel.
  In k8s, even if instance A attempts queued cancel while instance B attempts dispatch simultaneously,
  Lua atomicity ensures only one succeeds.

**Single-server environment**: operates identically. Lua ZREM atomicity is instance-count independent.

### 8. Gateway Intelligence — Automated Server Assignment

**Scale-Out** (horizontal expansion, per-model basis):
```
demand_counter(model) > eligible_capacity(model) × 0.80
  eligible_capacity = Σ max_concurrent(model, S)
                      for loaded S where:
                        !S.thermal_soft_gated && !S.thermal_hard_gated
                        && !S.circuit_open && !S.is_standby
                        && !ModelState(model, S).is_pulling
                        && !ModelState(model, S).dispatch_blocked
  // Models with dispatch_blocked==true cannot actually be dispatched by governor.
  // Including them in capacity calculation prevents Scale-Out condition from being met, suppressing expansion.
  // When governor is applying a cap, use effective max_concurrent = governor_cap(model, S).
  //   eligible_capacity = Σ governor_cap(model, S)  (governor-active servers)
  //                     + Σ max_concurrent(model, S) (governor-inactive servers)
  // total_capacity (all loaded included) must not be used: providers in soft/hard gate, pull, or CB open state
  // cannot accept new requests, so must be excluded from capacity to prevent Scale-Out misfires.
  // Example: Provider A loaded max=8 but Soft Gate → eligible=0.
  //          demand=6, eligible_capacity=0 → 0.80×0=0 → Scale-Out triggers correctly
→ target = argmax(free_vram, servers where !ModelState(model, S).is_loaded && Pass 0 candidates)
    // !is_loaded means "that model is not loaded on that server" — model+provider pair basis.
    // Servers where the same model is already loaded are excluded from Scale-Out (already serving).
    // is_standby servers are also in Pass 0 candidates, so selectable after ④ recovery.
    Tiebreak: if free_vram is equal, use provider_id ASC (deterministic order → prevents multi-instance split-brain)
    0 candidates (single-server environment): no-op (silently skip without Preloader call)
→ Valkey atomic claim before Preloader(target, model):
    Lua: SET preloading:{model}:{provider_id} 1 NX EX 180
    Returns nil = another instance already claimed → skip
    Reason: prevent duplicate Preload of same model+server across instances

**Claim lock lifecycle**:
  Acquired → Preloader executes
    Normal completion: DEL preloading:{model}:{provider_id} immediate release
                       (once is_loaded=true, next Scale-Out fails !is_loaded condition → auto-skip)
    Immediate failure (VramPool has_room not met): DEL immediate release → re-evaluate on next 5s loop
    timeout/error (within 3 attempts): DEL immediate release → preload_fail_count++ → retry allowed
    3 failures: DEL immediate release + set preload_failed_at → 300s exclusion (§4 rules)
    Veronex crash: TTL 180s natural expiry → other instances can retry

Post-Scale-Out hold-down: exclude that server from Scale-In candidates for 60s.
(Prevents overexpansion-overcontraction oscillation: Preload complete → queue drained → immediate Scale-In)
```

**Scale-In** (power savings, per-server basis):
```
server_idle(S): demand==0 for all loaded models AND active_requests==0 AND !last_server
Transient state protection: is_preloading==true OR within 30s of standby recovery → skip Scale-In
→ is_standby = true → Lazy Eviction fires naturally → Ollama memory released
```

**STANDBY → ACTIVE recovery**: demand > 0 detected → is_standby=false → routing resumes immediately.
If model is still loaded, serving starts immediately. If unloaded, Preloader reloads.

**Placement Planner loop (5s)**:

2-pass structure:
  [Pass 0 — Pre-computation (once at loop start)]
    scale_out_candidates = candidate_servers_for_scale_out()
      = {server | !server.thermal_soft_gated && !server.thermal_hard_gated
                  && !server.circuit_open
                  && free_vram(server) > 0}
      // Standby servers included (so ① can use them immediately after ④ recovery)
      // preload_failed_at is a model+provider pair attribute → must not filter at server set level.
      //   Per-model preload_failed_at check is done in ①② on a per-model basis.
      // is_pulling is also a model+provider pair attribute → likewise handled per-model in ①②.
      // thermal/CB must be excluded in Pass 0 so ④ does not select unusable servers as STANDBY recovery candidates.
    scale_out_needed = {model | demand(model) > total_capacity_excl_standby(model) × 0.80}
    // governor_cap / dispatch_blocked reflected in total_capacity calculation (see eligible_capacity §8 definition)

  Pass 0 makes no state changes (read-only). ④①②③⑤ all operate based on this snapshot.

  **Per-server provisional VRAM reservation** (prevents multi-model collision in same cycle):
    Copy each server's free_vram snapshot from Pass 0 into `provisional_free: HashMap<ProviderId, u32>`.
    Each time a server is selected as Preloader target in ①②:
      provisional_free[server] -= model.weight_mb + model.kv_per_request_mb
    When another model selects the same server in ①② of the same cycle:
      provisional_free[server] < model.weight_mb → exclude that server from candidates
    Reason: sharing Pass 0's free_vram snapshot lets multiple models select the same server simultaneously.
            Actual has_room check runs at Preloader execution time, but
            if planner causes artificial collisions, preload failure count increases → leads to 300s exclusion.
            Provisional reservation preemptively prevents collisions at the planner stage.
  **Multi-replica provisional_free non-determinism**:
    provisional_free is each replica's in-memory state, not shared across replicas.
    If two replicas simultaneously select different servers as best_server, the same model preloads on two servers.
    (NX lock is per {model}:{provider_id}, so different servers = different locks = both succeed)
    Result: excessive preload. Waste rather than load relief.
    Mitigation: Valkey `Scale-Out decision lock`: `SET scaleout:{model} {replica_id} NX EX 30`
      Returns nil = another replica is making Scale-Out decision for this model → skip.
      30s TTL = auto-expires before next Planner cycle. DEL after decision complete (preload NX lock acquired).

```
④ STANDBY recovery: when standby_server meets either condition → is_standby=false
     Condition A: server has is_loaded==true model with demand>0 (can serve immediately)
     Condition B: server selected as best_server in Pass 0 scale_out_needed × scale_out_candidates intersection
   (Reason: runs before ① but needs ①'s computation results → must pre-compute in Pass 0.
            Selectively recovers only servers that will actually serve or be used for Scale-Out, not `any demand>0`.)
① Scale-Out:   select best_server from Pass 0 candidates for scale_out_needed models
               (already set is_standby=false in ④, so included in total_capacity)
               && now_ms - preload_failed_at(model, best_server) >= 300_000
               → Preloader(best_server, model)
② Preload:     demand>0 && !is_loaded && !is_preloading && has_room
               && now_ms - preload_failed_at(model, provider) >= 300_000
               → Preloader
   (Reason: applies §4's 3-failure 300s exclusion directly to Planner loop, not just filter_candidates().
            Without this, Planner retries the same preload every 5s — 300s exclusion is nullified)
③ Evict:       demand==0 && is_loaded && active==0 &&
               (idle ≥ 180s  OR  (is_standby && idle ≥ 30s))
               → evict; sample_count=0
⑤ Scale-In:    server_idle && !last_server && !in_transition → is_standby=true

Collision prevention: servers selected as Scale-Out candidates in ① are excluded from Scale-In in ⑤ of the same cycle.
           Servers recovered in ④ have 30s transition guard → ⑤ skipped.
           ② and ⑤: servers targeted for Preload in ② are excluded from Scale-In in ⑤ of the same cycle.
           ③ and ④: models on servers recovered in ④ are excluded from ③ Evict candidates of the same cycle
                     (naturally excluded since idle condition is not met immediately after recovery).
Thermal integration: skip ①② when thermal hard_gate or soft_gate is active.
             (soft gate: loading additional models may push temperature toward hard_gate due to I/O load)
Pull integration: is_pulling is per ModelState (model+provider pair), not the entire ProviderVramState.
             Per-step exclusion rules:
             ① Scale-Out:  exclude only that model+provider pair from candidates. Other models on same provider unaffected.
             ② Preload:    exclude only that model+provider pair (reloading the same model during pull prohibited).
             ③ Evict:      model being pulled is excluded from evict. Other models on same provider can be evicted.
             ④/⑤ STANDBY/Scale-In: compute that model's capacity as 0 for the provider. Other models' capacity included normally.
Scale-Out deduplication:
  - Skip ① if count of servers with is_preloading==true && Thermal Normal ≥ needed_servers.
    needed_servers = ceil(demand_counter(model) / avg_max_concurrent(model))
    // Simple "skip if any preloading" serializes expansion during demand surges (2x demand but adding 1 server at a time).
    // needed_servers criterion allows concurrent Scale-Out to handle demand surges in parallel.
    // Single-server environment: needed_servers calculation is meaningless, just no-op.
  - Cleanup procedure when preloading server enters Soft/Hard state:
      1. Send cancel signal to that Preloader task (tokio CancellationToken)
      2. DEL preloading:{model}:{provider_id} (immediate Valkey NX lock release)
      3. is_preloading=false (VramPool ModelState atomic reset)
      Order guaranteed: 1→2→3. Lock must be released after cancel so other instances can reclaim immediately.
      Other server Scale-Out allowed afterward (re-evaluated on next 5s loop).
```

---

## End-to-End Data Flow

```
Client  POST /v1/chat/completions
    ▼
Veronex API  auth + rate limit → Job DB INSERT (status="queued")
    │  Lua enqueue_atomic: ZCARD < MAX_QUEUE_SIZE → ZADD + INCR demand  (atomic, 429 on queue full)
    │    On enqueue failure (429): DB job status → "failed", failure_reason = "queue_full" (prevent orphans)
    │    (Non-atomic ZCARD+ZADD prohibited — overshoot possible. See §2 MAX_QUEUE_SIZE atomicity spec)
    │  SSE heartbeat every 30s (keep connection alive while waiting for model load)
    ▼
Valkey ZSET
    │  Dispatcher loop (per instance)  ※ Migration: ZSET (primary) + LIST drain (secondary) in parallel — see Phase -1
    │  ZRANGE K=20 → Rust window scoring
    │  Lua atomic handoff: ZREM queue:zset + LPUSH processing + DECR demand:{model}
    │    Executed atomically as single script. Script failure (ZREM=0) = preempted → retry.
    │    No gap: job removed from ZSET is guaranteed to exist in processing.
    ▼
Dispatcher
    │  filter_candidates()  active + type + model + !is_standby
    │  score_and_claim()    CB → thermal_gate → AIMD gate → try_reserve()
    │                       CB takes priority, thermal hard_gate re-evaluated after CB release
    │  LREM processing (ACK) — demand DECR already completed in handoff above
    ▼
Job Runner → Ollama SSE → Client
    │  Response header: X-Job-Id: {job_id}  (included in initial 200 OK response)
    │  Reason: enables client to determine duplicate responses via job_id on at-least-once re-execution
    ▼
VramPermit drop  → KV released · last_active_at updated · PostgreSQL+ClickHouse recorded

[Startup]
  sync_providers_once()       ← /api/ps → update is_loaded (before dispatcher)
  recover_pending_jobs()      ← DB-based residual job recovery (DB scan instead of Valkey LRANGE)
                                 // Intentional improvement: Valkey is volatile, so DB is used as SSOT.
                                 //   Full recovery possible from DB even after Valkey crash/restart.
                                 1. DB scan: collect jobs with status IN ('pending', 'running')
                                    - status=pending: crash before or after queuing, before dispatch
                                    - status=running: crash after dispatch, before completion
                                 2. Re-queue each recovery target job via Lua atomic script:
                                    score = enqueued_at_ms - tier_bonus(job.tier)
                                    ZADD queue:zset {score} {job_id}
                                    HSET queue:model {job_id} {model}
                                    HSET queue:enqueue_at {job_id} {enqueued_at_ms}
                                 // DB-based, so LREM processing unnecessary — Valkey state rebuilt from scratch.
                                 Duplicate response prevention: check job status=completed/failed in DB,
                                 exclude already-terminal jobs from ZADD (at-least-once → idempotent)
                                 **TPM double-deduction prevention**: re-executed jobs do not re-deduct TPM.
                                   Skip deduction if DB job's reserved_tokens already exists.
                                 **API contract**: at-least-once. Duplicate responses possible on retry after crash.
                                   Response header X-Job-Id exposes job_id → enables client idempotency checks.
  resync_demand_counters()    ← recalculate demand_counter from actual ZSET state
  spawn dispatcher · placement_planner · sync_loop · demand_resync_loop(60s)
```

---

## Implementation Status

> **Complete**: full implementation + gap correction done (2026-03-14, SDD audit reflected)
> 2 intentional improvements, 4 accepted simplifications (G11-G14), 2 implemented (G15-G16)

### Core Implementation Status

| Item | Implementation location | Notes |
|------|----------|------|
| ZSET queue + Lua atomic scripts | `application/ports/outbound/valkey_port.rs` | enqueue/dispatch/cancel 3 scripts |
| MAX_QUEUE_SIZE = 10,000 / PER_MODEL = 2,000 | `domain/constants.rs` | atomic check (single Lua script) |
| Queue Full DB orphan prevention | `use_case.rs` | `Ok(false)` → cleanup jobs + DB status=Failed |
| AIMD (sample≥3, ratio<0.7/≥0.9, p95 spike) | `capacity/analyzer.rs` | baseline freeze on decay, 3-cycle stable update |
| LLM correction (stable_cycles≥3, **increase only**, +2 cap) | `capacity/analyzer.rs` | `change_floor = current` — decrease impossible |
| OOM response (safety_permil +50, max_concurrent ×3/4) | `capacity/vram_pool.rs` + `analyzer.rs` | -10 gradual recovery, max 500 |
| Thermal 5-state machine | `capacity/thermal.rs` | Normal/Soft/Hard/Cooldown/RampUp |
| Soft Gate release condition | `capacity/thermal.rs` | `temp < normal_below AND active_count == 0` |
| Hard Gate 60s forced drain | `ThermalDrainPort` + `ThermalDrainAdapter` | `placement_planner.rs` cancel on 60s elapsed |
| Hard Gate 90s watchdog | `placement_planner.rs` | error log + force timer set on 90s exceeded |
| Cooldown 300s + max 900s cap + temperature branching | `capacity/thermal.rs` | cooldown_secs × 3 absolute cap |
| RampUp max_concurrent=1 + accept requests | `inference/dispatcher.rs` | no 503 blocking unlike Soft |
| **RampUp → Normal: Σmax_concurrent ≥ pre_hard_total** | `capacity/thermal.rs` | snapshot saved on Hard entry, transition after restoration confirmed |
| Placement Planner 5s loop (Pass 0 + ④①②③⑤) | `placement_planner.rs` | provisional_free, Scale-Out decision lock |
| Preload 3-fail 300s exclusion | `capacity/vram_pool.rs` + dispatcher + planner | checked in both filter_candidates + ①② |
| Lazy Eviction (idle 180s / standby 30s) | `placement_planner.rs` | ③ Evict |
| promote_overdue (30s) | `queue_maintenance.rs` | HSCAN + ZADD XX EMERGENCY_BONUS |
| demand_resync (60s) | `queue_maintenance.rs` | ZSCAN → HMGET → recalculate |
| Multi-instance pub/sub + reaper | `pubsub/relay.rs`, `pubsub/reaper.rs` | crash recovery, orphan job re-queue |
| provider_vram_budget persistence | `provider_vram_budget_repository` | safety_permil DB storage |
| Startup recovery | `use_case.rs::recover_pending_jobs()` | DB scan based (instead of Valkey LRANGE) — intentional improvement |
| failure_reason column (G16) | `job_repository.rs::fail_with_reason()` | migration 000004 + all failure paths recorded |
| max_queue_wait 300s cancel (G15) | `queue_maintenance.rs::run_queue_wait_cancel_loop()` | 30s HSCAN → zset_cancel + DB fail |

### Accepted Simplifications

The following items work correctly but the implementation intentionally differs from the SDD original.

#### G11 — governor_cap not reflected in eligible_capacity

**Current implementation**: `placement_planner.rs` eligible_capacity calculation uses raw `max_concurrent` even for governor-active servers.

**Accepted because**:
- governor_cap of 0 (dispatch_blocked) is already excluded by `is_dispatch_blocked()` filter (line 191).
- governor_cap of 1~N (fair-share limit) may slightly underestimate actual dispatch throughput, causing Scale-Out to **trigger slightly earlier**.
- Early Scale-Out trigger is a safer error direction than Scale-Out suppression (conservative). No practical impact in single-server environment since it's no-op.

**Future refinement condition**: if governor is constantly active in multi-server environment and Scale-Out oscillation is observed, reflect `governor_cap` in eligible_capacity.

```rust
// current (simplified)
let mc = vram_pool.max_concurrent(p.id, &model);
// when refined
let cap = vram_pool.governor_cap(p.id, &model);
let mc  = if cap > 0 { cap } else { vram_pool.max_concurrent(p.id, &model) };
```

#### G12 — Preload task cleanup on Thermal Soft/Hard entry not implemented

**Current implementation**: in-progress Preloader tasks are not explicitly cancelled on Soft/Hard entry.

**Accepted because**:
- Placement Planner checks thermal state every 5s loop and **completely skips** ①② (Scale-Out, Preload) steps for servers in Soft/Hard/Cooldown (see thermal integration rules).
- Running Preloader tasks terminate naturally via their own timeout (120s) or success/failure. `is_preloading=false` + NX lock DEL always performed on termination.
- Preload HTTP requests may remain on Ollama for up to 120s, but Ollama receiving load in thermal Hard state is equivalent to having existing in-flight requests — VramPermit is not issued during preload, so no additional VRAM misuse.

**Future implementation condition**: if Preloader is measured to cause Ollama load even during Soft/Hard entry, add `active_preload_tokens: HashMap<(Uuid, String), CancellationToken>` to placement_planner for immediate cancellation.

#### G13 — Pull cancel DELETE endpoint not implemented

**Current implementation**: no `DELETE /v1/ollama/models/pull/{provider_id}/{model}` endpoint.

**Accepted because**:
- `max_pull_secs` (default 4h) timeout auto-releases hung pulls.
- Pull frequency is low (only on model replacement), so practical need for manual cancel is low.
- Emergency `is_pulling=false` force-clear possible via direct DB update.

**Future implementation condition**: if pull frequency increases or 4h timeout for large models (50GB+) becomes operationally burdensome.

#### G14 — Pull heartbeat_at monitoring not implemented

**Current implementation**: Ollama pull progress stream is not parsed to update `heartbeat_at`. Only `started_at + max_pull_secs` single timeout used.

**Accepted because**:
- Parsing Ollama pull progress stream adds complexity (JSON line parsing + 30s update logic) with limited benefit.
- `max_pull_secs` 4h cap sufficiently prevents hangs.
- heartbeat_at 300s stale detection provides finer judgment of "is pull actually progressing", but 4h cap prevents practical paralysis.

**Future implementation condition**: if network instability causes pulls to stall mid-stream while the connection stays open, measured in production.

#### G15 — max_queue_wait 300s background cancel loop

**Implemented**: `queue_maintenance.rs::run_queue_wait_cancel_loop()` (30s interval).
HSCAN `queue:enqueue_at` → detect jobs exceeding 300s → `zset_cancel()` + `fail_with_reason("queue_wait_exceeded")`.

#### G16 — failure_reason column

**Implemented**: migration `000004_add_failure_reason.{up,down}.sql` + `InferenceJob.failure_reason` field.
Per-failure values: `queue_full`, `no_eligible_provider`, `provider_error`, `token_budget_exceeded`, `queue_wait_exceeded`.

#### G17 — queue_wait_exceeded SSE error event not sent

**Current implementation**: `queue_wait_cancel_loop` only performs DB `failure_reason` + ZSET cancel. Does not push error event to waiting SSE clients.

**Accepted because**:
- SSE clients terminate naturally via connection timeout (browser default or app setting).
- On reconnection, `status=failed`, `failure_reason=queue_wait_exceeded` can be verified via `/v1/inference/{job_id}` query.
- queue_wait_cancel fires after 300s wait — most clients have already disconnected via timeout at this point.
- SSE push would require passing `DashMap<Uuid, Sender>` to queue_maintenance, significantly increasing module coupling.

**Future implementation condition**: if "no response" UX issues after 300s+ wait are measured in the frontend, add SSE error event delivery via `job_event_tx` broadcast.

---

## TDD — Testing Strategy

> Policy source: `docs/llm/policies/testing-strategy.md`
> Methodology: Testing Trophy + Contract Testing. **Integration-focused, no duplication, layer-separated responsibilities.**

### Per-Layer Test Responsibilities

| Layer | Verification target | Tools | Anti-Pattern |
|-------|----------|------|-------------|
| **Static** | Types, lint | Rust type system, Clippy | Do not write tests for type-catchable issues |
| **Unit** | Pure function logic | `cargo nextest`, `proptest` | No HTTP/DB verification |
| **Integration** | API contract (schema), port contract | mock port, OpenAPI validation | No duplication with E2E paths |
| **E2E** | User flows | bash e2e | No individual function verification |

**Test purity principle**: internal function change → only unit tests break → E2E unchanged.
If E2E breaks from internal function changes → **test design flaw** (layer violation).

### Test Writing Decision Checklist

```
1. Catchable by type system?    → Yes → No test needed (trust Rust type system)
2. Pure function?               → Yes → Unit (prefer proptest)
   ex. window_score(), perf_factor(), thermal state transition logic
3. External dependency? (Valkey/DB)  → Yes → Integration (use mock port)
   ex. Lua atomic scripts, Placement Planner port contract, ThermalDrainPort
4. User flow?                   → Yes → E2E (minimal only)
   ex. end-to-end inference, cancel flow, queue wait timeout
5. Verified in another layer?   → Yes → Do not write
```

### Scheduler Per-Component Test Scope

**Unit (`cargo nextest` + `proptest`)**:

| Component | Test target | Tools |
|----------|------------|------|
| `thermal.rs` | State machine transitions (Normal→Soft→Hard→Cooldown→RampUp), boundary values | proptest |
| `dispatcher.rs` | `window_score()`, `filter_candidates()` candidate filter | proptest |
| `valkey_keys.rs` | Key generation function correctness | cargo nextest |
| AIMD calculation | TPS ratio, p95 spike decay conditions, LLM correction direction constraints | proptest |
| `perf_factor()` | Per-temperature-zone linear interpolation (0.0~1.0) | proptest |

**Integration (mock port)**:

| Component | Test target |
|----------|------------|
| `ThermalDrainPort` | Verify `cancel_jobs_for_provider()` called when Hard Gate 60s exceeded |
| `VramPool` | `try_reserve()` / `release()` atomicity, standby flag isolation |
| Placement Planner | Drain contract verification via `ThermalDrainPort` mock |
| Lua scripts | enqueue/dispatch/cancel atomic handoff contracts (Valkey testcontainer) |

**E2E (bash scripts)**:

| Script | Verification flow |
|----------|----------|
| `02-inference.sh` | queued → processing → completed basic flow |
| `04-security.sh` | admin-only pull drain API permissions |
| `06-lifecycle.sh` | job cancel, queue wait timeout flow |

**cargo-mutants**: once before release. Prioritize Thermal state machine + AIMD core logic audit.

### Prohibited Test Patterns

```
✗ E2E breaks on Thermal state change → layer violation (unit responsibility)
✗ Direct DB verification of is_loaded / active_count → unit/integration responsibility
✗ Replacing Lua script logic with Rust mocks → contract hollowing (use Valkey testcontainer)
✗ Verifying window_score() in E2E → pure function, unit responsibility
```
