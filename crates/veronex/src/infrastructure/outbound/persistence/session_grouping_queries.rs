//! SSOT for the SQL behind the session-grouping background job.

use chrono::NaiveDate;
use sqlx::{PgPool, postgres::PgRow};
use uuid::Uuid;

/// Build the cutoff predicate. `Some(_)` → bind a date as $1; `None` →
/// inline `DATE_TRUNC('day', NOW())` (everything before today UTC).
pub fn cutoff_clause(has_cutoff: bool) -> &'static str {
    if has_cutoff {
        "AND created_at < $1::date::timestamptz"
    } else {
        "AND created_at < DATE_TRUNC('day', NOW())"
    }
}

/// Already-grouped jobs (within the 50 k cap), newest-first. Returned
/// rows expose `(api_key_id, account_id, messages_hash, conversation_id)`
/// via the same column names as the original handler-side query.
pub async fn fetch_grouped_jobs(
    pool: &PgPool,
    cutoff: Option<NaiveDate>,
) -> sqlx::Result<Vec<PgRow>> {
    let sql = format!(
        "SELECT api_key_id, account_id, messages_hash, conversation_id \
         FROM inference_jobs \
         WHERE conversation_id IS NOT NULL \
           AND messages_hash IS NOT NULL \
           {} \
         ORDER BY created_at DESC \
         LIMIT 50000",
        cutoff_clause(cutoff.is_some()),
    );
    if let Some(date) = cutoff {
        sqlx::query(&sql).bind(date).fetch_all(pool).await
    } else {
        sqlx::query(&sql).fetch_all(pool).await
    }
}

/// Ungrouped jobs (within the 10 k cap), oldest-first. Returns rows
/// containing `(id, api_key_id, account_id, messages_hash, messages_prefix_hash)`.
pub async fn fetch_ungrouped_jobs(
    pool: &PgPool,
    cutoff: Option<NaiveDate>,
) -> sqlx::Result<Vec<PgRow>> {
    let sql = format!(
        "SELECT id, api_key_id, account_id, messages_hash, messages_prefix_hash \
         FROM inference_jobs \
         WHERE conversation_id IS NULL \
           AND messages_hash IS NOT NULL \
           {} \
         ORDER BY created_at ASC \
         LIMIT 10000",
        cutoff_clause(cutoff.is_some()),
    );
    if let Some(date) = cutoff {
        sqlx::query(&sql).bind(date).fetch_all(pool).await
    } else {
        sqlx::query(&sql).fetch_all(pool).await
    }
}

/// Single-round-trip batch UPDATE via UNNEST. The two arrays must be
/// the same length; the index pairs each `job_id` with its
/// `conversation_id`.
pub async fn assign_conversations_batch(
    pool: &PgPool,
    job_ids: &[Uuid],
    conv_ids: &[String],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE inference_jobs AS j \
         SET conversation_id = u.conv_id \
         FROM UNNEST($1::uuid[], $2::text[]) AS u(job_id, conv_id) \
         WHERE j.id = u.job_id",
    )
    .bind(job_ids)
    .bind(conv_ids)
    .execute(pool)
    .await
    .map(|_| ())
}

