//! Background reaper tasks for multi-instance crash recovery.
//!
//! - **Heartbeat**: refreshes instance liveness key every 10s (EX 30s).
//! - **Slot reaper**: cleans up expired slot leases from crashed instances.
//! - **Queue reaper**: re-enqueues orphaned jobs from the processing list.
//!
//! ## Double-execution prevention
//!
//! All re-enqueue operations use Lua CAS scripts to prevent TOCTOU races:
//! - `LUA_REAP_OWNED_JOB`: atomically checks heartbeat + owner match before removing from processing.
//! - `LUA_REAP_OWNERLESS_JOB`: claims ownership with SET NX before removing from processing.
//! After Lua removal, the Rust caller re-enqueues to QUEUE_ZSET with model from DB.

use std::collections::HashSet;
use std::sync::Arc;

use fred::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP, TIER_BONUS_PAID};
use crate::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool;
use crate::infrastructure::outbound::valkey_keys;

/// Max entries to inspect per reap cycle — bounds LRANGE memory at 10K+ provider scale.
const REAP_CHUNK_SIZE: i64 = 500;

/// Lua CAS: atomically verify dead owner + remove from processing.
///
/// Only removes if `job:owner` still matches the expected (dead) instance
/// AND that instance's heartbeat is gone. Rust caller does the ZADD to QUEUE_ZSET.
///
/// KEYS[1] = job:owner:{job_id}
/// KEYS[2] = heartbeat:{instance_id}
/// KEYS[3] = veronex:queue:processing
/// ARGV[1] = job UUID string
/// ARGV[2] = expected dead instance_id
const LUA_REAP_OWNED_JOB: &str = r#"
local owner = redis.call('GET', KEYS[1])
if owner ~= ARGV[2] then
    return 0
end
local alive = redis.call('EXISTS', KEYS[2])
if alive == 1 then
    return 0
end
redis.call('LREM', KEYS[3], 1, ARGV[1])
redis.call('DEL', KEYS[1])
return 1
"#;

/// Lua CAS: claim ownerless job before removing from processing.
///
/// Uses SET NX to prevent multiple reapers from racing on the same ownerless job.
/// Rust caller does the ZADD to QUEUE_ZSET.
///
/// KEYS[1] = job:owner:{job_id}
/// KEYS[2] = veronex:queue:processing
/// ARGV[1] = job UUID string
const LUA_REAP_OWNERLESS_JOB: &str = r#"
local owner = redis.call('GET', KEYS[1])
if owner then
    return 0
end
local claimed = redis.call('SET', KEYS[1], 'reaper', 'NX', 'EX', 30)
if not claimed then
    return 0
end
redis.call('LREM', KEYS[2], 1, ARGV[1])
redis.call('DEL', KEYS[1])
return 1
"#;

/// Lua: ZADD to QUEUE_ZSET with emergency priority + update side hashes.
///
/// KEYS[1] = QUEUE_ZSET
/// KEYS[2] = demand key (veronex:demand:{model})
/// KEYS[3] = QUEUE_ENQUEUE_AT
/// KEYS[4] = QUEUE_MODEL_MAP
/// ARGV[1] = job UUID string
/// ARGV[2] = score (f64 as string)
/// ARGV[3] = enqueue_at ms
/// ARGV[4] = model name
const LUA_EMERGENCY_ENQUEUE: &str = r#"
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
local v = redis.call('INCR', KEYS[2])
if v < 0 then redis.call('SET', KEYS[2], 1) end
redis.call('HSET', KEYS[3], ARGV[1], ARGV[3])
redis.call('HSET', KEYS[4], ARGV[1], ARGV[4])
return 1
"#;

/// Combined reaper loop that runs heartbeat + slot reap + queue reap.
///
/// - Heartbeat: every 10s, SET heartbeat key with 30s TTL.
/// - Slot reap: every 30s, scan for expired leases.
/// - Queue reap: every 60s, check processing list for orphaned jobs.
pub async fn run_reaper_loop(
    pool: Pool,
    instance_id: Arc<str>,
    distributed_vram_pool: Option<Arc<DistributedVramPool>>,
    pg_pool: sqlx::PgPool,
    shutdown: CancellationToken,
) {
    let mut heartbeat_interval = tokio::time::interval(crate::domain::constants::REAPER_HEARTBEAT_INTERVAL);
    let mut reap_interval = tokio::time::interval(crate::domain::constants::REAPER_SLOT_INTERVAL);
    let mut queue_reap_interval = tokio::time::interval(crate::domain::constants::REAPER_QUEUE_INTERVAL);

    heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    reap_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    queue_reap_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tracing::info!(instance_id = %instance_id, "reaper loop started");

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = heartbeat_interval.tick() => {
                refresh_heartbeat(&pool, &instance_id).await;
            }
            _ = reap_interval.tick() => {
                if let Some(ref dvp) = distributed_vram_pool {
                    dvp.reap_all_expired().await;
                }
            }
            _ = queue_reap_interval.tick() => {
                reap_orphaned_jobs(&pool, &pg_pool).await;
            }
        }
    }

    tracing::info!("reaper loop stopped");
}

/// Refresh instance heartbeat (SET with 30s TTL) and register in instance set.
async fn refresh_heartbeat(pool: &Pool, instance_id: &str) {
    let key = valkey_keys::heartbeat(instance_id);
    let result: Result<(), _> = pool
        .set(&key, "1", Some(Expiration::EX(30)), None, false)
        .await;
    if let Err(e) = result {
        tracing::warn!("heartbeat refresh failed: {e}");
    }
    // Register in global instance set so orphan sweeper can enumerate all instances.
    let _: Result<i64, _> = pool.sadd(valkey_keys::INSTANCES_SET, instance_id).await;
}

/// Scan the processing list for jobs whose owner instance is dead, re-enqueue them.
///
/// Processes at most `REAP_CHUNK_SIZE` entries per cycle to bound Redis memory usage.
/// Uses batched MGET for owner and heartbeat lookups (2 round trips for the whole chunk),
/// then Lua CAS per confirmed-dead job. Collects reaped jobs and calls
/// `reenqueue_reaped_jobs_batch` for a single DB SELECT + single UPDATE.
async fn reap_orphaned_jobs(pool: &Pool, pg_pool: &sqlx::PgPool) {
    // Step 1: bounded LRANGE — at most REAP_CHUNK_SIZE entries per cycle.
    let entries: Vec<String> = match pool
        .lrange(valkey_keys::QUEUE_PROCESSING, 0, REAP_CHUNK_SIZE - 1)
        .await
    {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("queue reap: failed to read processing list: {e}");
            return;
        }
    };

    if entries.is_empty() {
        return;
    }

    // Step 2: filter valid UUIDs; evict garbage entries asynchronously.
    let jobs: Vec<(uuid::Uuid, String)> = entries
        .into_iter()
        .filter_map(|s| match uuid::Uuid::parse_str(&s) {
            Ok(u) => Some((u, s)),
            Err(_) => {
                let pool = pool.clone();
                let s_owned = s.clone();
                tokio::spawn(async move {
                    let _ = pool
                        .lrem::<i64, _, _>(valkey_keys::QUEUE_PROCESSING, 1, &s_owned)
                        .await;
                });
                None
            }
        })
        .collect();

    if jobs.is_empty() {
        return;
    }

    // Step 3: batch MGET all owner keys — 1 round trip.
    let owner_keys: Vec<String> = jobs
        .iter()
        .map(|(uuid, _)| valkey_keys::job_owner(*uuid))
        .collect();

    let owners: Vec<Option<String>> = pool
        .mget::<Vec<Option<String>>, _>(owner_keys.clone())
        .await
        .unwrap_or_default();

    // Step 4: collect unique non-nil owner instance IDs for heartbeat lookup.
    let unique_instances: Vec<String> = owners
        .iter()
        .filter_map(|o| o.as_deref())
        .collect::<HashSet<&str>>()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Step 5: batch MGET all heartbeat keys — 1 round trip.
    let alive_instances: HashSet<String> = if unique_instances.is_empty() {
        HashSet::new()
    } else {
        let hb_keys: Vec<String> = unique_instances
            .iter()
            .map(|id| valkey_keys::heartbeat(id))
            .collect();

        let hb_values: Vec<Option<String>> = pool
            .mget::<Vec<Option<String>>, _>(hb_keys)
            .await
            .unwrap_or_default();

        unique_instances
            .into_iter()
            .zip(hb_values.into_iter())
            .filter_map(|(id, val)| if val.is_some() { Some(id) } else { None })
            .collect()
    };

    // Step 6: per-job Lua CAS for confirmed-dead or ownerless jobs.
    // owners may be shorter than jobs if MGET returned fewer — zip with repeat(&None) as fallback.
    let mut reaped_jobs: Vec<(uuid::Uuid, String)> = Vec::new();

    for ((uuid, uuid_str), owner_opt) in jobs
        .iter()
        .zip(owners.iter().chain(std::iter::repeat(&None)))
    {
        let owner_key = valkey_keys::job_owner(*uuid);

        let reaped_ok = match owner_opt {
            Some(instance_id) => {
                // Skip if the owner instance is still alive.
                if alive_instances.contains(instance_id.as_str()) {
                    false
                } else {
                    let hb_key = valkey_keys::heartbeat(instance_id);
                    let result: Result<i64, _> = pool
                        .eval(
                            LUA_REAP_OWNED_JOB,
                            vec![
                                owner_key,
                                hb_key,
                                valkey_keys::QUEUE_PROCESSING.to_string(),
                            ],
                            vec![uuid_str.clone(), instance_id.clone()],
                        )
                        .await;
                    match result {
                        Ok(1) => {
                            tracing::info!(%uuid, %instance_id, "reaped orphaned job (CAS)");
                            true
                        }
                        Ok(_) => false, // owner changed or instance recovered — skip
                        Err(e) => {
                            tracing::warn!(%uuid, "reap CAS failed: {e}");
                            false
                        }
                    }
                }
            }
            None => {
                // Ownerless: atomic claim via SET NX.
                let result: Result<i64, _> = pool
                    .eval(
                        LUA_REAP_OWNERLESS_JOB,
                        vec![
                            owner_key,
                            valkey_keys::QUEUE_PROCESSING.to_string(),
                        ],
                        vec![uuid_str.clone()],
                    )
                    .await;
                match result {
                    Ok(1) => {
                        tracing::info!(%uuid, "reaped ownerless job (CAS)");
                        true
                    }
                    Ok(_) => false, // another reaper claimed it — skip
                    Err(e) => {
                        tracing::warn!(%uuid, "reap ownerless CAS failed: {e}");
                        false
                    }
                }
            }
        };

        if reaped_ok {
            reaped_jobs.push((*uuid, uuid_str.clone()));
        }
    }

    if reaped_jobs.is_empty() {
        return;
    }

    let reaped_count = reaped_jobs.len();
    reenqueue_reaped_jobs_batch(pool, pg_pool, reaped_jobs).await;
    tracing::info!(reaped = reaped_count, "reaper re-enqueued orphaned jobs to ZSET");
}

/// Batch re-enqueue reaped jobs: single DB SELECT + single UPDATE + per-job Lua ZADD.
///
/// Uses `ANY($1::uuid[])` to avoid N DB round trips regardless of batch size.
async fn reenqueue_reaped_jobs_batch(
    pool: &Pool,
    pg_pool: &sqlx::PgPool,
    reaped: Vec<(uuid::Uuid, String)>,
) {
    let ids: Vec<uuid::Uuid> = reaped.iter().map(|(id, _)| *id).collect();

    // Single SELECT for all model names.
    let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT id, model_name FROM inference_jobs WHERE id = ANY($1::uuid[])",
    )
    .bind(&ids as &[uuid::Uuid])
    .fetch_all(pg_pool)
    .await
    .unwrap_or_default();

    // Build id → model_name lookup.
    let model_map: std::collections::HashMap<uuid::Uuid, String> = rows
        .into_iter()
        .map(|(id, model)| (id, model))
        .collect();

    // Single batch UPDATE: reset all reaped running jobs to pending.
    let _ = sqlx::query(
        "UPDATE inference_jobs SET status = 'pending', started_at = NULL WHERE id = ANY($1::uuid[]) AND status = 'running'",
    )
    .bind(&ids as &[uuid::Uuid])
    .execute(pg_pool)
    .await;

    // Per-job Lua ZADD to QUEUE_ZSET with emergency priority (lowest score = highest priority).
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let score = now_ms.saturating_sub(TIER_BONUS_PAID) as f64;

    for (uuid, uuid_str) in &reaped {
        let model: &str = match model_map.get(uuid).map(String::as_str) {
            Some(m) => m,
            None => {
                tracing::warn!(%uuid, "reaped job not found in DB — skipping re-enqueue");
                continue;
            }
        };

        let demand_key = format!("veronex:demand:{model}");

        let result: Result<(), _> = pool
            .eval(
                LUA_EMERGENCY_ENQUEUE,
                vec![
                    QUEUE_ZSET.to_string(),
                    demand_key,
                    QUEUE_ENQUEUE_AT.to_string(),
                    QUEUE_MODEL_MAP.to_string(),
                ],
                vec![
                    uuid_str.clone(),
                    score.to_string(),
                    now_ms.to_string(),
                    model.to_string(),
                ],
            )
            .await;

        match result {
            Ok(()) => tracing::info!(%uuid, %model, "reaped job re-enqueued to QUEUE_ZSET"),
            Err(e) => tracing::warn!(%uuid, "failed to ZADD reaped job to QUEUE_ZSET: {e}"),
        }
    }
}
