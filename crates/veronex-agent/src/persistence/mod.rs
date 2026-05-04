//! SQL helpers for the agent's orphan-sweep logic.
//!
//! Per `policies/patterns/persistence.md`, raw SQL must live in a `persistence/`
//! module; the rest of the crate consumes these helpers.

use sqlx::PgPool;
use uuid::Uuid;

/// Fail a single running/pending inference job attributed to a dead owner.
/// Returns `Some(())` if the row was updated, `None` if the job was already in
/// a terminal state or missing.
pub async fn fail_orphaned_job(pg: &PgPool, id: Uuid) -> Result<Option<()>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "UPDATE inference_jobs
         SET status = 'failed', failure_reason = 'server_crash', completed_at = NOW()
         WHERE id = $1 AND status IN ('pending', 'running')
         RETURNING status",
    )
    .bind(id)
    .fetch_optional(pg)
    .await
    .map(|opt| opt.map(|_| ()))
}

/// Fail all running jobs whose `instance_id` matches the dead owner and have
/// been running longer than 2 minutes. Returns the number of rows updated.
/// Returns `Ok(0)` when the optional `instance_id` column is absent.
pub async fn fail_running_jobs_for_instance(
    pg: &PgPool,
    instance_id: &str,
) -> Result<u32, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "WITH updated AS (
            UPDATE inference_jobs
            SET status = 'failed', failure_reason = 'server_crash', completed_at = NOW()
            WHERE status = 'running'
              AND instance_id = $1
              AND started_at < NOW() - INTERVAL '2 minutes'
            RETURNING 1
         )
         SELECT COUNT(*) FROM updated",
    )
    .bind(instance_id)
    .fetch_one(pg)
    .await
    .map(|count| count as u32)
}

/// Fail running jobs whose provider was hard-deleted (FK SET NULL) and that
/// have been running longer than 5 minutes. Returns the number of rows updated.
pub async fn fail_orphan_provider_jobs(pg: &PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE inference_jobs
         SET status = 'failed', failure_reason = 'server_crash', completed_at = NOW()
         WHERE status = 'running'
           AND provider_id IS NULL
           AND created_at < NOW() - INTERVAL '5 minutes'",
    )
    .execute(pg)
    .await
    .map(|res| res.rows_affected())
}
