//! Background reaper tasks for multi-instance crash recovery.
//!
//! - **Heartbeat**: refreshes instance liveness key every 10s (EX 30s).
//! - **Slot reaper**: cleans up expired slot leases from crashed instances.
//! - **Queue reaper**: re-enqueues orphaned jobs from the processing list.
//!
//! ## Double-execution prevention
//!
//! All re-enqueue operations use Lua CAS scripts to prevent TOCTOU races:
//! - `LUA_REAP_OWNED_JOB`: atomically checks heartbeat + owner match before re-enqueue.
//! - `LUA_REAP_OWNERLESS_JOB`: claims ownership with SET NX before re-enqueue,
//!   preventing multiple reapers from racing on the same job.

use std::sync::Arc;

use fred::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool;
use crate::infrastructure::outbound::valkey_keys;

/// Lua CAS: atomically verify dead owner + re-enqueue.
///
/// Prevents double execution: only re-enqueues if `job:owner` still matches
/// the expected (dead) instance AND that instance's heartbeat is gone.
///
/// KEYS[1] = job:owner:{job_id}
/// KEYS[2] = heartbeat:{instance_id}
/// KEYS[3] = veronex:queue:processing
/// KEYS[4] = veronex:queue:jobs (target re-enqueue queue)
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
redis.call('RPUSH', KEYS[4], ARGV[1])
redis.call('DEL', KEYS[1])
return 1
"#;

/// Lua CAS: claim ownerless job before re-enqueue.
///
/// Uses SET NX to prevent multiple reapers from racing on the same ownerless job.
///
/// KEYS[1] = job:owner:{job_id}
/// KEYS[2] = veronex:queue:processing
/// KEYS[3] = veronex:queue:jobs (target re-enqueue queue)
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
redis.call('RPUSH', KEYS[3], ARGV[1])
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
                reap_orphaned_jobs(&pool).await;
            }
        }
    }

    tracing::info!("reaper loop stopped");
}

/// Refresh instance heartbeat (SET with 30s TTL).
async fn refresh_heartbeat(pool: &Pool, instance_id: &str) {
    let key = valkey_keys::heartbeat(instance_id);
    let result: Result<(), _> = pool
        .set(&key, "1", Some(Expiration::EX(30)), None, false)
        .await;
    if let Err(e) = result {
        tracing::warn!("heartbeat refresh failed: {e}");
    }
}

/// Scan the processing list for jobs whose owner instance is dead, re-enqueue them.
///
/// Uses Lua CAS scripts to prevent double-execution: each re-enqueue is atomic
/// (check owner + heartbeat + LREM + RPUSH + DEL in one Lua eval).
async fn reap_orphaned_jobs(pool: &Pool) {
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

        match owner {
            Some(instance_id) => {
                let hb_key = valkey_keys::heartbeat(&instance_id);

                // Atomic CAS: re-enqueue only if owner still matches AND heartbeat is dead.
                let result: Result<i64, _> = pool
                    .eval(
                        LUA_REAP_OWNED_JOB,
                        vec![
                            owner_key,
                            hb_key,
                            valkey_keys::QUEUE_PROCESSING.to_string(),
                            valkey_keys::QUEUE_JOBS.to_string(),
                        ],
                        vec![uuid_str.clone(), instance_id.clone()],
                    )
                    .await;
                match result {
                    Ok(1) => {
                        tracing::info!(%uuid, %instance_id, "reaped orphaned job (CAS)");
                        reaped += 1;
                    }
                    Ok(_) => {} // owner changed or instance recovered — skip
                    Err(e) => tracing::warn!(%uuid, "reap CAS failed: {e}"),
                }
            }
            None => {
                // Atomic CAS: claim ownerless job via SET NX before re-enqueue.
                let result: Result<i64, _> = pool
                    .eval(
                        LUA_REAP_OWNERLESS_JOB,
                        vec![
                            owner_key,
                            valkey_keys::QUEUE_PROCESSING.to_string(),
                            valkey_keys::QUEUE_JOBS.to_string(),
                        ],
                        vec![uuid_str.clone()],
                    )
                    .await;
                match result {
                    Ok(1) => {
                        tracing::info!(%uuid, "reaped ownerless job (CAS)");
                        reaped += 1;
                    }
                    Ok(_) => {} // another reaper claimed it — skip
                    Err(e) => tracing::warn!(%uuid, "reap ownerless CAS failed: {e}"),
                }
            }
        }
    }

    if reaped > 0 {
        tracing::info!(reaped, "reaper re-enqueued orphaned jobs");
    }
}
