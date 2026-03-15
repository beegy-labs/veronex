use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;

/// Timeout for acquiring a connection from the pool.
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// Create a PostgreSQL connection pool from the given URL.
///
/// Pool size is configurable via `PG_POOL_MAX` env var (default: 10).
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let max_conns: u32 = std::env::var("PG_POOL_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_conns)
        .min_connections(2)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .connect(database_url)
        .await?;
    Ok(pool)
}
