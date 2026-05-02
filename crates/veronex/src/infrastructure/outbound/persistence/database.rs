use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;

/// Timeout for acquiring a connection from the pool.
/// Must be shorter than the HTTP timeout so callers fail fast on pool exhaustion.
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(600);
const POOL_MAX_LIFETIME: Duration = Duration::from_secs(1800);

/// Create a PostgreSQL connection pool from the given URL.
///
/// `max_conns` is sourced from `AppConfig::pg_pool_max` (env `PG_POOL_MAX`,
/// default 10) so all env reads stay in `bootstrap::config`.
pub async fn connect(database_url: &str, max_conns: u32) -> Result<PgPool> {
    use sqlx::postgres::PgConnectOptions;
    use std::str::FromStr;

    // statement_cache_capacity is on PgConnectOptions, not PgPoolOptions.
    // 512 (up from default 100) reduces parse/plan overhead per connection.
    let connect_options = PgConnectOptions::from_str(database_url)?
        .statement_cache_capacity(512);

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_conns)
        .min_connections(2)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .idle_timeout(POOL_IDLE_TIMEOUT)
        .max_lifetime(POOL_MAX_LIFETIME)
        .test_before_acquire(false)
        .connect_with(connect_options)
        .await?;
    Ok(pool)
}
