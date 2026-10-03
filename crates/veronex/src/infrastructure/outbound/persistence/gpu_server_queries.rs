//! Ad-hoc SSOT queries for `gpu_servers` that don't fit the
//! `GpuServerRegistry` port (which is entity-shaped).

use sqlx::PgPool;

/// True when at least one row already binds to `url` — used by the
/// register/probe handlers to reject duplicate URLs before a more
/// expensive HTTP probe.
pub async fn url_is_registered(pool: &PgPool, url: &str) -> sqlx::Result<bool> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM gpu_servers WHERE node_exporter_url = $1")
            .bind(url)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}
