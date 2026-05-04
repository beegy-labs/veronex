# Crash Recovery & Job Reaping

> **Last Updated**: 2026-03-28

---

## Overview

Background reaper loop handles instance crash recovery via heartbeats,
VRAM lease cleanup, and orphaned job re-enqueue. All re-enqueue operations
use Lua CAS scripts to prevent TOCTOU double-execution races.

---

## Combined Reaper Loop

```
run_reaper_loop:
  loop tokio::select! (biased):
    shutdown      → break
    heartbeat_interval (10s) → refresh_heartbeat()
    reap_interval      (30s) → dvp.reap_all_expired()   // VRAM leases
    queue_reap_interval(60s) → reap_orphaned_jobs()
```

---

## Heartbeat

```
refresh_heartbeat(instance_id):
  SET heartbeat:{instance_id} "1" EX 30
  SADD veronex:instances instance_id
```

If an instance crashes, its heartbeat key expires after 30s.
The `INSTANCES_SET` allows other instances to enumerate all known peers.

---

## Slot Reaper (VRAM Leases)

```
every 30s:
  DistributedVramPool.reap_all_expired()
  // scans slot lease keys, removes expired leases from crashed instances
```

---

## Queue Reaper (Orphaned Jobs)

```
reap_orphaned_jobs():
  1. LRANGE veronex:queue:processing 0 499     // bounded chunk
  2. filter valid UUIDs (evict garbage async)
  3. batch MGET job:owner:{id} for all jobs     // 1 round trip
  4. collect unique owner instance_ids
  5. batch MGET heartbeat:{instance_id}          // 1 round trip
  6. per confirmed-dead job:
       if has owner:
         EVAL LUA_REAP_OWNED_JOB               // CAS: check owner + heartbeat
       else (ownerless):
         EVAL LUA_REAP_OWNERLESS_JOB            // CAS: SET NX claim + LREM
  7. reenqueue_reaped_jobs_batch()
```

---

## Lua CAS Scripts

```
LUA_REAP_OWNED_JOB:
  if GET job:owner:{id} != expected_instance → return 0
  if EXISTS heartbeat:{instance} → return 0    // still alive
  LREM processing 1 job_id
  DEL job:owner:{id}
  return 1

LUA_REAP_OWNERLESS_JOB:
  if GET job:owner:{id} exists → return 0      // someone claimed it
  SET job:owner:{id} "reaper" NX EX 30         // atomic claim
  if not claimed → return 0
  LREM processing 1 job_id
  DEL job:owner:{id}
  return 1
```

---

## Batch Re-enqueue

```
reenqueue_reaped_jobs_batch(reaped):
  SELECT id, model_name FROM inference_jobs WHERE id = ANY($1::uuid[])
  UPDATE status='pending', started_at=NULL WHERE id = ANY(...) AND status='running'
  per job:
    EVAL LUA_EMERGENCY_ENQUEUE:
      ZADD QUEUE_ZSET score job_id           // emergency priority
      INCR demand:{model}
      HSET queue:enqueue_at job_id now_ms
      HSET queue:model_map job_id model
  score = now_ms - TIER_BONUS_PAID           // highest priority
```

---

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `REAPER_HEARTBEAT_INTERVAL` | 10s | Heartbeat refresh rate |
| `REAPER_SLOT_INTERVAL` | 30s | VRAM lease reap interval |
| `REAPER_QUEUE_INTERVAL` | 60s | Orphaned job reap interval |
| Heartbeat TTL | 30s (EX) | Instance considered dead after 30s silence |
| `REAP_CHUNK_SIZE` | 500 | Max processing list entries per cycle |
| `TIER_BONUS_PAID` | score bonus | Emergency re-enqueue priority |

---

## Files

| File | Role |
|------|------|
| `crates/veronex/src/infrastructure/outbound/pubsub/reaper.rs` | Reaper loop, Lua scripts, re-enqueue |
| `crates/veronex/src/infrastructure/outbound/capacity/distributed_vram_pool.rs` | `reap_all_expired()` |
| `crates/veronex/src/domain/constants.rs` | Canonical Valkey key constructors + interval/timing constants — SSOT |
| `crates/veronex/src/infrastructure/outbound/valkey_keys.rs` | pk-aware shims for direct-fred callers |
| `crates/veronex/src/bootstrap/background.rs` | Reaper task spawn |
