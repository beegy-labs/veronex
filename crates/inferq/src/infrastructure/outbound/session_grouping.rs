/// Session grouping loop — runs once per day in the background.
///
/// Groups inference_jobs into conversations by matching message chains:
///   job B's messages_prefix_hash == job A's messages_hash
///   → same api_key_id / account_id → same conversation
///
/// No LLM needed — pure hash comparison.
/// No race conditions — works on already-completed jobs.
///
/// Algorithm (in-memory, O(n)):
///   1. Fetch existing hash → conversation_id mapping.
///   2. Fetch ungrouped jobs (conversation_id IS NULL) sorted by created_at.
///   3. For each job: prefix_hash == "" → new session; match → inherit; else → new session.
///   4. Batch UPDATE via UNNEST.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// ── Public entry point ────────────────────────────────────────────────────────

pub async fn run_session_grouping_loop(
    pg_pool:  Arc<PgPool>,
    interval: Duration,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown.cancelled() => {
                tracing::info!("session grouping loop shutting down");
                return;
            }
        }

        match group_sessions(&pg_pool).await {
            Ok(n) if n > 0 => tracing::info!(grouped = n, "session grouping complete"),
            Ok(_)          => tracing::debug!("session grouping: nothing to group"),
            Err(e)         => tracing::warn!("session grouping failed: {e}"),
        }
    }
}

// ── Core logic ────────────────────────────────────────────────────────────────

async fn group_sessions(pg_pool: &PgPool) -> anyhow::Result<usize> {
    // 1. Load existing hash → conversation_id for already-grouped jobs.
    //    Key: (api_key_id, account_id, messages_hash) → prevents cross-key contamination.
    let existing_rows = sqlx::query(
        "SELECT api_key_id, account_id, messages_hash, conversation_id
         FROM inference_jobs
         WHERE conversation_id IS NOT NULL
           AND messages_hash IS NOT NULL
         ORDER BY created_at DESC
         LIMIT 50000",
    )
    .fetch_all(pg_pool)
    .await?;

    let mut hash_to_conv: HashMap<(Option<Uuid>, Option<Uuid>, String), String> =
        HashMap::with_capacity(existing_rows.len());

    for row in &existing_rows {
        let api_key_id: Option<Uuid>  = row.try_get("api_key_id").unwrap_or(None);
        let account_id: Option<Uuid>  = row.try_get("account_id").unwrap_or(None);
        let messages_hash: Option<String> = row.try_get("messages_hash").unwrap_or(None);
        let conversation_id: String   = row.try_get("conversation_id").unwrap_or_default();
        if let Some(h) = messages_hash {
            hash_to_conv
                .entry((api_key_id, account_id, h))
                .or_insert(conversation_id);
        }
    }

    // 2. Fetch ungrouped jobs oldest-first so chains resolve in order.
    let ungrouped_rows = sqlx::query(
        "SELECT id, api_key_id, account_id, messages_hash, messages_prefix_hash
         FROM inference_jobs
         WHERE conversation_id IS NULL
           AND messages_hash IS NOT NULL
         ORDER BY created_at ASC
         LIMIT 10000",
    )
    .fetch_all(pg_pool)
    .await?;

    if ungrouped_rows.is_empty() {
        return Ok(0);
    }

    // 3. Assign conversation_ids in-memory.
    let mut ids_to_update: Vec<(Uuid, String)> = Vec::with_capacity(ungrouped_rows.len());

    for row in &ungrouped_rows {
        let job_id:     Uuid           = row.try_get("id")?;
        let api_key_id: Option<Uuid>   = row.try_get("api_key_id").unwrap_or(None);
        let account_id: Option<Uuid>   = row.try_get("account_id").unwrap_or(None);
        let messages_hash: Option<String>   = row.try_get("messages_hash").unwrap_or(None);
        let prefix_hash:   Option<String>   = row.try_get("messages_prefix_hash").unwrap_or(None);

        let prefix = match &prefix_hash {
            Some(p) => p.as_str(),
            None    => continue, // no messages — skip
        };

        let conversation_id = if prefix.is_empty() {
            // First turn — start a new conversation.
            Uuid::now_v7().to_string()
        } else {
            let key = (api_key_id, account_id, prefix.to_string());
            match hash_to_conv.get(&key) {
                Some(conv) => conv.clone(),
                None => {
                    // Orphan (parent outside the 50k window or has no hash).
                    Uuid::now_v7().to_string()
                }
            }
        };

        // Register this job's hash so subsequent jobs in the same chain find it.
        if let Some(h) = messages_hash {
            hash_to_conv.insert(
                (api_key_id, account_id, h),
                conversation_id.clone(),
            );
        }

        ids_to_update.push((job_id, conversation_id));
    }

    if ids_to_update.is_empty() {
        return Ok(0);
    }

    // 4. Batch UPDATE — single round-trip via UNNEST.
    let job_ids:  Vec<Uuid>   = ids_to_update.iter().map(|(id, _)| *id).collect();
    let conv_ids: Vec<String> = ids_to_update.iter().map(|(_, c)| c.clone()).collect();

    sqlx::query(
        "UPDATE inference_jobs AS j
         SET conversation_id = u.conv_id
         FROM UNNEST($1::uuid[], $2::text[]) AS u(job_id, conv_id)
         WHERE j.id = u.job_id",
    )
    .bind(&job_ids)
    .bind(&conv_ids)
    .execute(pg_pool)
    .await?;

    Ok(ids_to_update.len())
}
