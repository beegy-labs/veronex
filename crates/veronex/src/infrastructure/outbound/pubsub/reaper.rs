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

use std::sync::Arc;

use fred::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP, TIER_BONUS_PAID};
use crate::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool;
use crate::infrastructure::outbound::valkey_keys;

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
/// Uses Lua CAS scripts to prevent double-execution: each LREM from processing is atomic
/// (check owner + heartbeat + LREM + DEL in one Lua eval). After Lua, re-enqueues to
/// QUEUE_ZSET with emergency priority by looking up the model from DB.
async fn reap_orphaned_jobs(pool: &Pool, pg_pool: &sqlx::PgPool) {
    let entries: Vec<String> = match pool.lrange(valkey_keys::QUEUE_PROCESSING, 0, -1).await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("queue reap: failed to read processing list: {e}");
            return;
        }
    };

    if entries.is_empty() {
        return;
    }

    let mut reaped = 0u32;
    for uuid_str in &entries {
        let uuid = match uuid::Uuid::parse_str(uuid_str) {
            Ok(u) => u,
            Err(_) => {
                let _ = pool.lrem::<i64, _, _>(valkey_keys::QUEUE_PROCESSING, 1, uuid_str).await;
                continue;
            }
        };

        let owner_key = valkey_keys::job_owner(uuid);
        let owner: Option<String> = pool.get(&owner_key).await.unwrap_or(None);

        let reaped_ok = match owner {
            Some(instance_id) => {
                let hb_key = valkey_keys::heartbeat(&instance_id);
                // Atomic CAS: LREM from processing only if owner matches AND heartbeat is dead.
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
                    Ok(1) => { tracing::info!(%uuid, %instance_id, "reaped orphaned job (CAS)"); true }
                    Ok(_) => false, // owner changed or instance recovered — skip
                    Err(e) => { tracing::warn!(%uuid, "reap CAS failed: {e}"); false }
                }
            }
            None => {
                // Atomic CAS: claim ownerless job via SET NX before removing from processing.
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
                    Ok(1) => { tracing::info!(%uuid, "reaped ownerless job (CAS)"); true }
                    Ok(_) => false, // another reaper claimed it — skip
                    Err(e) => { tracing::warn!(%uuid, "reap ownerless CAS failed: {e}"); false }
                }
            }
        };

        if !reaped_ok {
            continue;
        }

        // Re-enqueue to QUEUE_ZSET: look up model from DB, ZADD with emergency priority.
        reenqueue_reaped_job(pool, pg_pool, uuid, uuid_str).await;
        reaped += 1;
    }

    if reaped > 0 {
        tracing::info!(reaped, "reaper re-enqueued orphaned jobs to ZSET");
    }
}

/// Look up job model from DB and ZADD to QUEUE_ZSET with emergency priority.
async fn reenqueue_reaped_job(
    pool: &Pool,
    pg_pool: &sqlx::PgPool,
    uuid: uuid::Uuid,
    uuid_str: &str,
) {
    // Fetch model_name from DB (not in Valkey model_map after dispatch).
    let row = sqlx::query_scalar::<_, String>(
        "SELECT model_name FROM inference_jobs WHERE id = $1"
    )
    .bind(uuid)
    .fetch_optional(pg_pool)
    .await;

    let model = match row {
        Ok(Some(m)) => m,
        Ok(None) => {
            tracing::warn!(%uuid, "reaped job not found in DB — skipping re-enqueue");
            return;
        }
        Err(e) => {
            tracing::warn!(%uuid, "DB lookup for reaped job failed: {e}");
            return;
        }
    };

    // Reset DB status to pending so the job shows correctly while queued.
    let _ = sqlx::query(
        "UPDATE inference_jobs SET status = 'pending', started_at = NULL WHERE id = $1 AND status = 'running'"
    )
    .bind(uuid)
    .execute(pg_pool)
    .await;

    // ZADD QUEUE_ZSET with emergency priority (lowest score = highest priority).
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let score = now_ms.saturating_sub(TIER_BONUS_PAID) as f64;
    let demand_key = format!("veronex:demand:{}", model);

    // ZADD + side-hash updates (enqueue_at + model_map) — mirror of LUA_ZSET_ENQUEUE
    // but without the capacity guard (reaped jobs get emergency admission).
    let result: Result<(), _> = pool
        .eval(
            r#"
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
local v = redis.call('INCR', KEYS[2])
if v < 0 then redis.call('SET', KEYS[2], 1) end
redis.call('HSET', KEYS[3], ARGV[1], ARGV[3])
redis.call('HSET', KEYS[4], ARGV[1], ARGV[4])
return 1
"#,
            vec![
                QUEUE_ZSET.to_string(),
                demand_key,
                QUEUE_ENQUEUE_AT.to_string(),
                QUEUE_MODEL_MAP.to_string(),
            ],
            vec![
                uuid_str.to_string(),
                score.to_string(),
                now_ms.to_string(),
                model.clone(),
            ],
        )
        .await;

    match result {
        Ok(()) => tracing::info!(%uuid, %model, "reaped job re-enqueued to QUEUE_ZSET"),
        Err(e) => tracing::warn!(%uuid, "failed to ZADD reaped job to QUEUE_ZSET: {e}"),
    }
}

