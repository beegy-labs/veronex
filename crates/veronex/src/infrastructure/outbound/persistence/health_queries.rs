//! SSOT for the SQL behind the health-checker probes.

use sqlx::PgPool;
use uuid::Uuid;

/// Single-row liveness probe used by the readiness handler.
pub async fn ping(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

/// Persist a freshly-detected GPU vendor on the server row, but only
/// when it differs from the current value (avoids an UPDATE per scrape
/// cycle when nothing changed).
pub async fn update_gpu_vendor_if_changed(
    pool: &PgPool,
    server_id: Uuid,
    vendor: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE gpu_servers SET gpu_vendor = $1 WHERE id = $2 AND gpu_vendor != $1")
        .bind(vendor)
        .bind(server_id)
        .execute(pool)
        .await
        .map(|_| ())
}
