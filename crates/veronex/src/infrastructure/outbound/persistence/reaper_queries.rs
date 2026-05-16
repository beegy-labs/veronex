//! SSOT for the SQL behind the orphan-job reaper.

use sqlx::PgPool;
use uuid::Uuid;

/// Single round-trip lookup of `(id, model_name)` for the reaped job
/// batch. Bounded by the reaper's batch cap.
pub async fn fetch_model_names(
    pool: &PgPool,
    ids: &[Uuid],
) -> sqlx::Result<Vec<(Uuid, String)>> {
    sqlx::query_as("SELECT id, model_name FROM inference_jobs WHERE id = ANY($1::uuid[])")
        .bind(ids)
        .fetch_all(pool)
        .await
}

/// Reset every reaped row that is still `running` back to `pending` so
/// it can be redispatched.
pub async fn reset_reaped_to_pending(pool: &PgPool, ids: &[Uuid]) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE inference_jobs SET status = 'pending', started_at = NULL \
         WHERE id = ANY($1::uuid[]) AND status = 'running'",
    )
    .bind(ids)
    .execute(pool)
    .await
    .map(|_| ())
}
