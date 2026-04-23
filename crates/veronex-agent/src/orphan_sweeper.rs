//! Orphan job sweeper — detects crashed API instances and cleans up their jobs.
//!
//! ## Flow
//!
//! 1. Every 30s, read `veronex:instances` (SET of all API instance IDs).
//! 2. For each instance, check `veronex:heartbeat:{id}`:
//!    - Alive → clear suspect marker → skip.
//!    - Dead  → start/check 2-minute suspect grace period.
//! 3. After 2 minutes confirmed dead → atomically claim with `reaped:{id}` NX
//!    → fail orphaned jobs in DB → SREM from instance set.
//!
//! ## Shard distribution
//!
//! Each agent replica only processes instances it owns (`hash(id) % replicas == ordinal`),
//! enabling 10K+ server scale without coordination.
//!
//! ## Leader sweep (CronJob)
//!
//! A separate leader-elected sweep catches orphan jobs from deleted/inactive providers
//! not covered by the per-instance sweep.

use std::time::Duration;

use fred::clients::Pool;
use fred::prelude::*;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use crate::shard;

// ── Valkey key patterns ────────────────────────────────────────────────────

const INSTANCES_SET: &str = "veronex:instances";
const JOBS_RUNNING_COUNTER: &str = "veronex:stats:jobs:running";
const JOBS_PENDING_COUNTER: &str = "veronex:stats:jobs:pending";

fn heartbeat_key(instance_id: &str) -> String {
    format!("veronex:heartbeat:{instance_id}")
}

fn suspect_key(instance_id: &str) -> String {
    format!("veronex:suspect:{instance_id}")
}

fn reaped_key(instance_id: &str) -> String {
    format!("veronex:reaped:{instance_id}")
}

const ORPHAN_CRON_LOCK: &str = "veronex:orphan-cron:lock";

/// Suspect marker TTL: 3 minutes. We check after 2 minutes (TTL <= 60).
const SUSPECT_TTL_SECS: i64 = 180;

/// After 2 minutes, TTL drops to 60s or below → confirmed dead.
const SUSPECT_CONFIRM_THRESHOLD: i64 = 60;

/// Reaped marker TTL: 24 hours (prevents re-processing).
const REAPED_TTL_SECS: i64 = 86400;

/// Leader lock TTL for CronJob sweep.
const CRON_LOCK_TTL_SECS: i64 = 90;

/// Sweep interval.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// Leader sweep interval.
const LEADER_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

// ── Public entry point ──────────────────────────────────────────────────────

/// Run the orphan sweeper loop. Spawns both shard sweep and leader sweep.
pub async fn run_orphan_sweeper(
    valkey: Pool,
    pg: PgPool,
    ordinal: u32,
    replicas: u32,
    shutdown: CancellationToken,
) {
    let mut sweep_interval = tokio::time::interval(SWEEP_INTERVAL);
    let mut leader_interval = tokio::time::interval(LEADER_SWEEP_INTERVAL);
    sweep_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    leader_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tracing::info!(ordinal, replicas, "orphan sweeper started");

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = sweep_interval.tick() => {
                if let Err(e) = sweep_once(&valkey, &pg, ordinal, replicas).await {
                    tracing::warn!(error = %e, "orphan sweep failed");
                }
            }
            _ = leader_interval.tick() => {
                if let Err(e) = leader_sweep(&valkey, &pg).await {
                    tracing::warn!(error = %e, "orphan leader sweep failed");
                }
            }
        }
    }

    tracing::info!("orphan sweeper stopped");
}

// ── Shard sweep ─────────────────────────────────────────────────────────────

async fn sweep_once(
    valkey: &Pool,
    pg: &PgPool,
    ordinal: u32,
    replicas: u32,
) -> anyhow::Result<()> {
    let instances: Vec<String> = valkey.smembers(INSTANCES_SET).await?;
    if instances.is_empty() {
        return Ok(());
    }

    for instance_id in &instances {
        // Shard filter: only process instances this agent owns.
        if !shard::owns(instance_id, ordinal, replicas) {
            continue;
        }

        let hb_key = heartbeat_key(instance_id);
        let exists: bool = valkey.exists(&hb_key).await.unwrap_or(false);

        if exists {
            // Instance is alive — clear suspect marker if any.
            let s_key = suspect_key(instance_id);
            let _: Result<i64, _> = valkey.del(&s_key).await;
            continue;
        }

        // Heartbeat missing — check suspect state.
        let s_key = suspect_key(instance_id);
        let ttl: i64 = valkey.ttl(&s_key).await.unwrap_or(-2);

        if ttl == -2 {
            // No suspect marker — start grace period.
            let _: Result<(), _> = valkey
                .set(
                    &s_key,
                    "1",
                    Some(Expiration::EX(SUSPECT_TTL_SECS)),
                    None,
                    false,
                )
                .await;
            tracing::info!(instance_id, "instance suspect — grace period started");
            continue;
        }

        if ttl > SUSPECT_CONFIRM_THRESHOLD {
            // Less than 2 minutes elapsed — wait.
            continue;
        }

        // 2+ minutes elapsed — confirmed dead.
        // Atomically claim cleanup with NX.
        let r_key = reaped_key(instance_id);
        let claimed: Option<String> = valkey
            .set(
                &r_key,
                "1",
                Some(Expiration::EX(REAPED_TTL_SECS)),
                Some(SetOptions::NX),
                false,
            )
            .await
            .unwrap_or(None);

        if claimed.is_none() {
            // Another agent already claimed — skip.
            continue;
        }

        tracing::warn!(instance_id, "instance confirmed dead — cleaning up orphaned jobs");
        cleanup_instance(valkey, pg, instance_id).await;
    }

    Ok(())
}

/// Clean up orphaned jobs for a confirmed-dead instance.
async fn cleanup_instance(valkey: &Pool, pg: &PgPool, instance_id: &str) {
    // Find jobs owned by this dead instance from Valkey processing list.
    let processing: Vec<String> = valkey
        .lrange("veronex:queue:processing", 0, -1)
        .await
        .unwrap_or_default();

    let mut cleaned_running = 0u32;
    let cleaned_pending = 0u32;

    for uuid_str in &processing {
        let owner_key = format!("veronex:job:owner:{uuid_str}");
        let owner: Option<String> = valkey.get(&owner_key).await.unwrap_or(None);

        if owner.as_deref() != Some(instance_id) {
            continue;
        }

        // This job was owned by the dead instance — fail it.
        let uuid = match uuid::Uuid::parse_str(uuid_str) {
            Ok(u) => u,
            Err(_) => continue,
        };

        match crate::persistence::fail_orphaned_job(pg, uuid).await {
            Ok(Some(_)) => {
                cleaned_running += 1;
                // Clean up Valkey state.
                let _: Result<i64, _> =
                    valkey.lrem("veronex:queue:processing", 1, uuid_str).await;
                let _: Result<i64, _> = valkey.del(&owner_key).await;
            }
            Ok(None) => {} // Already cleaned or status changed.
            Err(e) => {
                tracing::warn!(%uuid, error = %e, "orphan sweeper: failed to update job");
            }
        }
    }

    // Also catch jobs that might not be in the processing list but are in DB
    // with this instance as owner (belt-and-suspenders).
    let db_cleaned = match crate::persistence::fail_running_jobs_for_instance(pg, instance_id).await
    {
        Ok(count) => count,
        Err(e) => {
            // instance_id column may not exist — that's OK, skip.
            tracing::debug!(error = %e, "orphan sweeper: DB instance_id sweep skipped");
            0
        }
    };

    let total = cleaned_running + cleaned_pending + db_cleaned;

    // Fire-and-forget DECR counters (single INCR with negative value instead of N round-trips).
    if cleaned_running > 0 {
        let _: Result<i64, _> = valkey.incr_by(JOBS_RUNNING_COUNTER, -(cleaned_running as i64)).await;
    }
    if cleaned_pending > 0 {
        let _: Result<i64, _> = valkey.incr_by(JOBS_PENDING_COUNTER, -(cleaned_pending as i64)).await;
    }

    // Remove from instance set and clear suspect marker.
    let _: Result<i64, _> = valkey.srem(INSTANCES_SET, instance_id).await;
    let _: Result<i64, _> = valkey.del(suspect_key(instance_id)).await;

    if total > 0 {
        tracing::info!(instance_id, total, cleaned_running, db_cleaned, "orphan cleanup complete");
    } else {
        tracing::info!(instance_id, "orphan cleanup complete (no orphaned jobs found)");
    }
}

// ── Leader sweep ────────────────────────────────────────────────────────────

/// Leader-elected sweep for orphan jobs from deleted/inactive providers.
/// Only one agent acquires the lock per cycle.
async fn leader_sweep(valkey: &Pool, pg: &PgPool) -> anyhow::Result<()> {
    // Try to acquire leader lock (NX + EX).
    let locked: Option<String> = valkey
        .set(
            ORPHAN_CRON_LOCK,
            "1",
            Some(Expiration::EX(CRON_LOCK_TTL_SECS)),
            Some(SetOptions::NX),
            false,
        )
        .await
        .unwrap_or(None);

    if locked.is_none() {
        return Ok(()); // Another agent is leader this cycle.
    }

    // Find orphan jobs: RUNNING jobs whose provider was hard-deleted
    // (FK ON DELETE SET NULL clears provider_id), older than 5 minutes.
    //
    // NOTE: pending jobs always have provider_id=NULL before dispatch — they
    // are NOT orphans. Only running jobs are expected to have a non-NULL
    // provider_id, so a NULL here means the provider was deleted mid-run.
    let result = crate::persistence::fail_orphan_provider_jobs(pg).await;

    match result {
        Ok(count) if count > 0 => {
            tracing::info!(
                count,
                "leader sweep: failed orphaned jobs from inactive providers"
            );
            // Counters will be reconciled by the stats ticker (every 60 ticks).
        }
        Err(e) => {
            tracing::warn!(error = %e, "leader sweep: DB update failed");
        }
        _ => {}
    }

    Ok(())
}
