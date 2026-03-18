//! Queue maintenance background loops (Phase 4 + G15).
//!
//! Three loops maintain ZSET queue integrity:
//! - `promote_overdue`: 30s — upgrades long-waiting jobs to prevent starvation
//! - `demand_resync`: 60s — reconciles demand counters + GC stale side-hash entries
//! - `queue_wait_cancel`: 30s — cancels jobs exceeding MAX_QUEUE_WAIT_SECS (§7 G15)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use fred::prelude::*;
use fred::types::scan::Scanner;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::constants::{
    EMERGENCY_BONUS_MS, MAX_QUEUE_WAIT_SECS, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP,
    QUEUE_ZSET, TIER_EXPIRE_SECS,
};
use crate::infrastructure::outbound::valkey_keys::JOBS_PENDING_COUNTER;

// ── Promote Overdue ─────────────────────────────────────────────────────────

/// Background loop: every `interval` seconds, scan `queue:enqueue_at` side hash
/// and upgrade jobs waiting longer than `TIER_EXPIRE_SECS` with EMERGENCY_BONUS.
///
/// This prevents starvation of lower-tier requests under continuous paid load.
/// Uses ZADD XX (update-only) to avoid re-inserting already-dispatched jobs.
pub async fn run_promote_overdue_loop(
    pool: Pool,
    interval: Duration,
    shutdown: CancellationToken,
) {
    tracing::info!("promote_overdue loop started (interval={}s)", interval.as_secs());

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }

        if let Err(e) = promote_overdue_pass(&pool).await {
            tracing::warn!("promote_overdue pass failed: {e}");
        }
    }

    tracing::info!("promote_overdue loop stopped");
}

async fn promote_overdue_pass(pool: &Pool) -> anyhow::Result<()> {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let threshold_ms = TIER_EXPIRE_SECS * 1000;
    let mut promoted = 0_u32;

    // HSCAN queue:enqueue_at — streaming cursor scan via fred 10 API
    let stream = pool.next().hscan(QUEUE_ENQUEUE_AT, "*", Some(200));
    futures::pin_mut!(stream);

    while let Some(result) = stream.next().await {
        let mut page = result?;
        if let Some(map) = page.take_results() {
            for (key, value) in map.inner() {
                let job_id: String = match key.as_str() {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let enqueue_at_ms: u64 = match value.as_string().and_then(|s| s.parse().ok()) {
                    Some(v) => v,
                    None => continue,
                };

                let wait_ms = now_ms.saturating_sub(enqueue_at_ms);
                if wait_ms > threshold_ms {
                    let new_score = enqueue_at_ms.saturating_sub(EMERGENCY_BONUS_MS) as f64;
                    // ZADD XX: update only if member exists (no re-insert)
                    let _: i64 = pool
                        .zadd(
                            QUEUE_ZSET,
                            Some(SetOptions::XX),
                            None,
                            false,
                            false,
                            (new_score, job_id.as_str()),
                        )
                        .await
                        .unwrap_or(0);
                    promoted += 1;
                }
            }
        }
    }

    if promoted > 0 {
        tracing::info!(promoted, "overdue jobs promoted with EMERGENCY_BONUS");
    }

    Ok(())
}

// ── Demand Resync ───────────────────────────────────────────────────────────

/// Background loop: every `interval` seconds, reconcile demand counters against
/// actual ZSET membership and GC stale side-hash entries.
///
/// ZSET is the single source of truth. demand counters are SET (not INCR) to
/// the actual count derived from ZSCAN + HMGET.
pub async fn run_demand_resync_loop(
    pool: Pool,
    interval: Duration,
    shutdown: CancellationToken,
) {
    tracing::info!("demand_resync loop started (interval={}s)", interval.as_secs());

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }

        if let Err(e) = demand_resync_pass(&pool).await {
            tracing::warn!("demand_resync pass failed: {e}");
        }
    }

    tracing::info!("demand_resync loop stopped");
}

async fn demand_resync_pass(pool: &Pool) -> anyhow::Result<()> {
    // 1. ZSCAN queue:zset — collect all job_ids (ZSET = single source of truth)
    let mut zset_members: Vec<String> = Vec::new();
    let stream = pool.next().zscan(QUEUE_ZSET, "*", Some(200));
    futures::pin_mut!(stream);

    while let Some(result) = stream.next().await {
        let mut page = result?;
        if let Some(entries) = page.take_results() {
            for (value, _score) in entries {
                if let Some(member) = value.as_string() {
                    zset_members.push(member);
                }
            }
        }
    }

    // 2. Batch HMGET queue:model for all ZSET members → model lookup
    let mut model_counts: HashMap<String, u64> = HashMap::new();

    for chunk in zset_members.chunks(200) {
        let keys: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        let models: Vec<Option<String>> = pool.hmget(QUEUE_MODEL_MAP, keys).await?;

        for model_opt in models.into_iter().flatten() {
            *model_counts.entry(model_opt).or_insert(0) += 1;
        }
    }

    // 3. SET demand:{model} to actual count (overwrite any drift)
    for (model, count) in &model_counts {
        let key = crate::domain::constants::demand_key(model);
        let _: () = pool.set(&key, count.to_string(), None, None, false).await?;
    }

    // 4. Stale GC: HSCAN queue:model — remove entries not in ZSET
    let zset_set: std::collections::HashSet<&str> =
        zset_members.iter().map(|s| s.as_str()).collect();

    gc_stale_hash(pool, QUEUE_MODEL_MAP, &zset_set).await;
    gc_stale_hash(pool, QUEUE_ENQUEUE_AT, &zset_set).await;

    if !model_counts.is_empty() {
        tracing::debug!(
            models = model_counts.len(),
            zset_size = zset_members.len(),
            "demand_resync completed"
        );
    }

    Ok(())
}

// ── Queue Wait Cancel (G15 — SDD §7) ────────────────────────────────────

/// Background loop: every `interval` seconds, scan `queue:enqueue_at` side hash
/// and cancel jobs waiting longer than `MAX_QUEUE_WAIT_SECS` (300s).
///
/// Jobs are atomically removed from the ZSET via the ValkeyPort cancel script
/// and marked as failed in the database with failure_reason = "queue_wait_exceeded".
pub async fn run_queue_wait_cancel_loop(
    pool: Pool,
    valkey: Arc<dyn ValkeyPort>,
    job_repo: Arc<dyn JobRepository>,
    interval: Duration,
    shutdown: CancellationToken,
) {
    tracing::info!(
        "queue_wait_cancel loop started (interval={}s, max_wait={}s)",
        interval.as_secs(),
        MAX_QUEUE_WAIT_SECS,
    );

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }

        if let Err(e) = queue_wait_cancel_pass(&pool, &valkey, &job_repo).await {
            tracing::warn!("queue_wait_cancel pass failed: {e}");
        }
    }

    tracing::info!("queue_wait_cancel loop stopped");
}

async fn queue_wait_cancel_pass(
    pool: &Pool,
    valkey: &Arc<dyn ValkeyPort>,
    job_repo: &Arc<dyn JobRepository>,
) -> anyhow::Result<()> {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let threshold_ms = MAX_QUEUE_WAIT_SECS * 1000;
    let mut cancelled = 0_u32;

    // HSCAN queue:enqueue_at — find jobs older than MAX_QUEUE_WAIT_SECS
    let stream = pool.next().hscan(QUEUE_ENQUEUE_AT, "*", Some(200));
    futures::pin_mut!(stream);

    while let Some(result) = stream.next().await {
        let mut page = result?;
        if let Some(map) = page.take_results() {
            for (key, value) in map.inner() {
                let job_id_str: String = match key.as_str() {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let enqueue_at_ms: u64 = match value.as_string().and_then(|s| s.parse().ok()) {
                    Some(v) => v,
                    None => continue,
                };

                let wait_ms = now_ms.saturating_sub(enqueue_at_ms);
                if wait_ms <= threshold_ms {
                    continue;
                }

                // Look up the model from side hash for ZSET cancel
                let model: String = pool
                    .hget(QUEUE_MODEL_MAP, &job_id_str)
                    .await
                    .unwrap_or_default();

                // Atomic ZSET cancel (ZREM + DECR demand + HDEL side hashes)
                match valkey.zset_cancel(&job_id_str, &model).await {
                    Ok(true) => {
                        // Mark DB as failed with reason
                        let uuid = match uuid::Uuid::parse_str(&job_id_str) {
                            Ok(u) => u,
                            Err(_) => continue,
                        };
                        let job_id = crate::domain::value_objects::JobId(uuid);
                        if let Err(e) = job_repo.fail_with_reason(
                            &job_id,
                            "queue_wait_exceeded",
                            Some("queue wait exceeded 300s"),
                        ).await {
                            tracing::warn!(%uuid, "failed to persist queue_wait_exceeded: {e}");
                        }
                        // pending → failed (queue_wait_exceeded): DECR pending
                        if let Err(e) = valkey.incr_by(JOBS_PENDING_COUNTER, -1).await {
                            tracing::warn!("DECR pending counter failed: {e}");
                        }
                        cancelled += 1;
                        tracing::info!(
                            %uuid, wait_secs = wait_ms / 1000,
                            "queue_wait_exceeded — job cancelled after {}s", wait_ms / 1000,
                        );
                    }
                    Ok(false) => {
                        // Already dispatched — processing cancel will handle it
                    }
                    Err(e) => {
                        tracing::warn!(job_id = %job_id_str, "ZSET cancel failed: {e}");
                    }
                }
            }
        }
    }

    if cancelled > 0 {
        tracing::info!(cancelled, "queue_wait_cancel: expired jobs removed");
    }

    Ok(())
}

/// Remove hash entries whose keys are not in the ZSET member set.
async fn gc_stale_hash(
    pool: &Pool,
    hash_key: &str,
    valid_members: &std::collections::HashSet<&str>,
) {
    let mut stale_count = 0_u32;
    let stream = pool.next().hscan(hash_key, "*", Some(200));
    futures::pin_mut!(stream);

    while let Some(result) = stream.next().await {
        let mut page = match result {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(hash_key, "stale GC hscan failed: {e}");
                return;
            }
        };

        if let Some(map) = page.take_results() {
            for (field, _value) in map.inner() {
                let field_str: &str = match field.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                if !valid_members.contains(field_str) {
                    let _: Result<i64, _> = pool.hdel(hash_key, field_str).await;
                    stale_count += 1;
                }
            }
        }
    }

    if stale_count > 0 {
        tracing::info!(hash_key, stale_count, "stale hash entries removed");
    }
}
