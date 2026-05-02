//! Queue maintenance background loops (Phase 4 + G15).
//!
//! Three loops maintain ZSET queue integrity:
//! - `promote_overdue`: 30s — upgrades long-waiting jobs to prevent starvation
//! - `demand_resync`: 60s — reconciles demand counters + GC stale side-hash entries
//! - `queue_wait_cancel`: 30s — cancels jobs exceeding MAX_QUEUE_WAIT_SECS (§7 G15)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use fred::prelude::*;
use fred::types::scan::Scanner;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::constants::{EMERGENCY_BONUS_MS, MAX_QUEUE_WAIT_SECS, TIER_EXPIRE_SECS};
use crate::infrastructure::outbound::valkey_keys as vk;

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
    let queue_enqueue_at = vk::queue_enqueue_at();
    let queue_zset = vk::queue_zset();

    // HSCAN queue:enqueue_at — streaming cursor scan via fred 10 API
    let stream = pool.next().hscan(&queue_enqueue_at, "*", Some(200));
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
                            &queue_zset,
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
    let queue_zset = vk::queue_zset();
    let queue_model_map = vk::queue_model_map();
    let queue_enqueue_at = vk::queue_enqueue_at();

    // ZCARD guard: skip entirely if queue is tiny (no significant drift possible)
    let zcard: u64 = pool.zcard(&queue_zset).await.unwrap_or(0);
    if zcard < 50 {
        return Ok(());
    }

    let mut model_counts: HashMap<String, u64> = HashMap::new();
    let mut zset_set: HashSet<String> = HashSet::new();

    // Stream ZSCAN and process each page directly with HMGET (no pre-allocation of all members)
    let stream = pool.next().zscan(&queue_zset, "*", Some(200));
    futures::pin_mut!(stream);

    while let Some(result) = stream.next().await {
        let mut page = result?;
        if let Some(entries) = page.take_results() {
            // Collect this page's job_ids
            let page_ids: Vec<String> = entries
                .into_iter()
                .filter_map(|(value, _score)| value.as_string())
                .collect();

            if page_ids.is_empty() {
                continue;
            }

            // Batch HMGET for this page immediately — no global Vec accumulation
            let keys: Vec<&str> = page_ids.iter().map(|s| s.as_str()).collect();
            let models: Vec<Option<String>> = pool.hmget(&queue_model_map, keys).await?;

            for model_opt in models.into_iter().flatten() {
                *model_counts.entry(model_opt).or_insert(0) += 1;
            }

            // Build GC set during the same stream pass
            for id in page_ids {
                zset_set.insert(id);
            }
        }
    }

    // SET demand:{model} to actual count (overwrite any drift)
    for (model, count) in &model_counts {
        let key = vk::demand_counter(model);
        let _: () = pool.set(&key, count.to_string(), None, None, false).await?;
    }

    // Stale GC: HSCAN side hashes — remove entries not in ZSET
    gc_stale_hash(pool, &queue_model_map, &zset_set).await;
    gc_stale_hash(pool, &queue_enqueue_at, &zset_set).await;

    if !model_counts.is_empty() {
        tracing::debug!(
            models = model_counts.len(),
            zset_size = zset_set.len(),
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

    // Pass 1: HSCAN queue:enqueue_at — collect expired (job_id, enqueue_at_ms) pairs
    let mut expired: Vec<(String, u64)> = Vec::new();
    let queue_enqueue_at = vk::queue_enqueue_at();
    let queue_model_map = vk::queue_model_map();
    let stream = pool.next().hscan(&queue_enqueue_at, "*", Some(200));
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
                if wait_ms > threshold_ms {
                    expired.push((job_id_str, enqueue_at_ms));
                }
            }
        }
    }

    if expired.is_empty() {
        return Ok(());
    }

    // Pass 2: single HMGET to fetch all models for expired jobs
    let expired_ids: Vec<&str> = expired.iter().map(|(id, _)| id.as_str()).collect();
    let models: Vec<Option<String>> = pool.hmget(&queue_model_map, expired_ids).await?;

    // Pass 3: process each cancellation
    for ((job_id_str, enqueue_at_ms), model_opt) in expired.iter().zip(models.into_iter()) {
        let model: String = model_opt.unwrap_or_default();
        let wait_ms = now_ms.saturating_sub(*enqueue_at_ms);

        match valkey.zset_cancel(job_id_str, &model).await {
            Ok(true) => {
                let uuid = match uuid::Uuid::parse_str(job_id_str) {
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
                if let Err(e) = valkey.incr_by(crate::domain::constants::JOBS_PENDING_COUNTER_KEY, -1).await {
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

    if cancelled > 0 {
        tracing::info!(cancelled, "queue_wait_cancel: expired jobs removed");
    }

    Ok(())
}

// ── Processing Reaper ────────────────────────────────────────────────────────

/// Background loop: every `interval` seconds, find jobs in queue:active whose
/// lease has expired (score < now_ms) and re-enqueue or permanently fail them.
pub async fn run_processing_reaper_loop(
    valkey: Arc<dyn ValkeyPort>,
    job_repo: Arc<dyn JobRepository>,
    interval: Duration,
    shutdown: CancellationToken,
) {
    tracing::info!("processing_reaper loop started (interval={}s)", interval.as_secs());
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }
        if let Err(e) = processing_reaper_pass(&valkey, &job_repo).await {
            tracing::warn!("processing_reaper pass failed: {e}");
        }
    }
    tracing::info!("processing_reaper loop stopped");
}

async fn processing_reaper_pass(
    valkey: &Arc<dyn ValkeyPort>,
    job_repo: &Arc<dyn JobRepository>,
) -> anyhow::Result<()> {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let expired = valkey.active_lease_expired(now_ms).await?;
    if expired.is_empty() {
        return Ok(());
    }

    tracing::info!(count = expired.len(), "processing_reaper: found expired leases");

    for job_id_str in &expired {
        let uuid = match uuid::Uuid::parse_str(job_id_str) {
            Ok(u) => u,
            Err(_) => {
                valkey.active_lease_remove(job_id_str).await.ok();
                continue;
            }
        };
        let job_id = crate::domain::value_objects::JobId(uuid);

        let attempts_key = format!("{}:{}", crate::domain::constants::QUEUE_ACTIVE_ATTEMPTS, job_id_str);
        let attempts: u64 = valkey
            .kv_get(&attempts_key)
            .await
            .unwrap_or(None)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if attempts >= crate::domain::constants::LEASE_MAX_ATTEMPTS {
            // Too many retries → fail permanently
            if let Err(e) = job_repo
                .fail_with_reason(
                    &job_id,
                    "lease_expired_max_attempts",
                    Some("lease expired after max retry attempts"),
                )
                .await
            {
                tracing::warn!(%uuid, "reaper: fail_with_reason error: {e}");
            }
            valkey.active_lease_remove(job_id_str).await.ok();
            valkey.kv_del(&attempts_key).await.ok();
            tracing::warn!(%uuid, "processing_reaper: job failed after max attempts");
        } else {
            // Re-enqueue: remove from active, bump attempt counter, push back into ZSET
            valkey.active_lease_remove(job_id_str).await.ok();
            valkey
                .kv_set(&attempts_key, &(attempts + 1).to_string(), 86400, false)
                .await
                .ok();

            let score = chrono::Utc::now().timestamp_millis() as f64;
            match job_repo.get(&job_id).await {
                Ok(Some(job)) => {
                    let model = job.model_name.as_str();
                    let now_ms2 = chrono::Utc::now().timestamp_millis() as u64;
                    match valkey
                        .zset_enqueue(uuid, score, model, now_ms2, u64::MAX, u64::MAX)
                        .await
                    {
                        Ok(_) => tracing::info!(
                            %uuid,
                            attempt = attempts + 1,
                            "processing_reaper: job re-enqueued"
                        ),
                        Err(e) => {
                            tracing::warn!(%uuid, "processing_reaper: re-enqueue failed: {e}");
                            job_repo
                                .fail_with_reason(
                                    &job_id,
                                    "lease_expired_reenqueue_failed",
                                    None,
                                )
                                .await
                                .ok();
                        }
                    }
                }
                _ => {
                    tracing::warn!(%uuid, "processing_reaper: job not found in DB, skipping");
                    valkey.active_lease_remove(job_id_str).await.ok();
                }
            }
        }
    }

    Ok(())
}

/// Remove hash entries whose keys are not in the ZSET member set.
/// Batches HDEL per HSCAN page to avoid per-entry round trips.
async fn gc_stale_hash(
    pool: &Pool,
    hash_key: &str,
    valid_members: &HashSet<String>,
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
            // Collect all stale fields in this page, then issue a single HDEL
            let stale_fields: Vec<String> = map
                .inner()
                .iter()
                .filter_map(|(field, _value)| {
                    let s = field.as_str()?;
                    if !valid_members.contains(s) {
                        Some(s.to_string())
                    } else {
                        None
                    }
                })
                .collect();

            if !stale_fields.is_empty() {
                stale_count += stale_fields.len() as u32;
                if let Err(e) = pool.hdel::<i64, _, _>(hash_key, stale_fields).await {
                    tracing::warn!(error = %e, "Valkey HDEL stale model-map fields failed");
                }
            }
        }
    }

    if stale_count > 0 {
        tracing::info!(hash_key, stale_count, "stale hash entries removed");
    }
}
