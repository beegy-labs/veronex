use anyhow::Result;
use sqlx::PgPool;

/// Create a PostgreSQL connection pool from the given URL.
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}
