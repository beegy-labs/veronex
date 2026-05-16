//! SSOT for the job-status-counter reconciliation SQL used by the
//! startup seed and the periodic stats ticker.

use sqlx::PgPool;

/// `(status, count)` for every job currently in pending/running state.
/// Returns at most 2 rows. Caller maps the pair into Valkey counters.
pub async fn count_active_by_status(pool: &PgPool) -> Vec<(String, i64)> {
    sqlx::query_as::<_, (String, i64)>(
        "SELECT status::text, COUNT(*) FROM inference_jobs \
         WHERE status IN ('pending','running') GROUP BY status",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}
