# Queue Maintenance Loops

> **Last Updated**: 2026-04-07

---

## Overview

Four background loops maintain ZSET priority queue integrity:
promote overdue jobs, reconcile demand counters, cancel stale waiters,
and reap expired active leases.

---

## Loop Summary

```
┌────────────────────┬──────────┬──────────────────────────────────────────┐
│ Loop               │ Interval │ Purpose                                  │
├────────────────────┼──────────┼──────────────────────────────────────────┤
│ promote_overdue    │ 30s      │ Anti-starvation score upgrade            │
│ demand_resync      │ 60s      │ Demand counter + GC side hashes          │
│ queue_wait_cancel  │ 30s      │ Cancel jobs waiting > 300s               │
│ processing_reaper  │ 30s      │ Reap expired queue:active leases         │
└────────────────────┴──────────┴──────────────────────────────────────────┘
```

---

## promote_overdue

Prevents starvation of lower-tier jobs under continuous paid-tier load.

```
promote_overdue_pass():
  now_ms = now()
  HSCAN queue:enqueue_at (cursor, count=200):
    for (job_id, enqueue_at_ms):
      wait_ms = now_ms - enqueue_at_ms
      if wait_ms > TIER_EXPIRE_SECS * 1000:
        new_score = enqueue_at_ms - EMERGENCY_BONUS_MS
        ZADD QUEUE_ZSET XX new_score job_id    // XX = update-only
```

`ZADD XX` ensures already-dispatched jobs are not re-inserted.

---

## demand_resync

ZSET is the single source of truth. Demand counters are overwritten
to the actual count derived from ZSCAN + HMGET.

```
demand_resync_pass():
  if ZCARD < 50 → skip                         // no significant drift

  ZSCAN QUEUE_ZSET (cursor, count=200):
    per page:
      page_ids = collect job_ids
      models = HMGET queue:model_map page_ids   // batch per page
      accumulate model_counts[model]++
      add page_ids to zset_set

  for (model, count) in model_counts:
    SET demand:{model} count                    // overwrite drift

  gc_stale_hash(queue:model_map, zset_set)      // HSCAN + batch HDEL
  gc_stale_hash(queue:enqueue_at, zset_set)
```

---

## queue_wait_cancel (G15)

Cancels jobs waiting longer than `MAX_QUEUE_WAIT_SECS` in the queue.

```
queue_wait_cancel_pass():
  Pass 1 — collect expired:
    HSCAN queue:enqueue_at (cursor, count=200):
      if now_ms - enqueue_at_ms > MAX_QUEUE_WAIT_SECS * 1000:
        expired.push(job_id, enqueue_at_ms)

  Pass 2 — batch model lookup:
    HMGET queue:model_map expired_ids

  Pass 3 — cancel each:
    valkey.zset_cancel(job_id, model)            // atomic ZREM
    job_repo.fail_with_reason("queue_wait_exceeded")
    DECR JOBS_PENDING_COUNTER
```

---

## processing_reaper

Reclaims jobs whose active lease expired (worker died or stalled).
Re-enqueues up to `LEASE_MAX_ATTEMPTS` times, then permanently fails the job.

```
processing_reaper_pass():
  now_ms = now()
  expired = ZRANGEBYSCORE queue:active 0 now_ms    // score = deadline_ms

  for job_id in expired:
    attempts = GET queue:active:attempts:{job_id} ?? 0
    if attempts >= LEASE_MAX_ATTEMPTS:
      job_repo.fail_with_reason("lease_expired_max_attempts")
      ZREM queue:active job_id
      DEL queue:active:attempts:{job_id}
    else:
      ZREM queue:active job_id
      SET queue:active:attempts:{job_id} attempts+1 EX 86400
      zset_enqueue(job_id, now_ms_score, model, ...)   // back into queue:zset
```

---

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `OVERDUE_PROMOTE_SECS` | 30 | Promote loop interval |
| `DEMAND_RESYNC_SECS` | 60 | Resync loop interval |
| `QUEUE_WAIT_CANCEL_SECS` | 30 | Wait-cancel loop interval |
| `PROCESSING_REAPER_SECS` | 30 | Processing reaper interval |
| `TIER_EXPIRE_SECS` | 250 | Wait threshold for promotion |
| `EMERGENCY_BONUS_MS` | 300,000 | Score bonus for overdue jobs |
| `MAX_QUEUE_WAIT_SECS` | 300 | Max queue wait before cancel |
| `LEASE_TTL_MS` | 90,000 | Active lease lifetime (ms) |
| `LEASE_RENEW_INTERVAL_SECS` | 30 | Worker keepalive cadence |
| `LEASE_MAX_ATTEMPTS` | 2 | Max re-enqueues before perm fail |
| ZCARD skip threshold | 50 | Demand resync guard |
| HSCAN page size | 200 | Cursor scan batch size |

---

## Files

| File | Role |
|------|------|
| `crates/veronex/src/infrastructure/outbound/queue_maintenance.rs` | All three loops |
| `crates/veronex/src/domain/constants.rs` | Timing/threshold constants |
| `crates/veronex/src/bootstrap/background.rs` | Task spawn wiring |
| `crates/veronex/src/infrastructure/outbound/valkey_keys.rs` | Queue key names |
