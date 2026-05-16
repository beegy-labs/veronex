//! Ad-hoc SSOT queries for `llm_providers` that don't fit the
//! `LlmProviderRegistry` port (which is entity-shaped).

use sqlx::PgPool;

/// True when at least one llama-server provider already binds to `url`.
/// Used by the register/probe handlers to reject duplicate URLs before
/// running the more expensive `/health` probe.
pub async fn llama_server_url_is_registered(pool: &PgPool, url: &str) -> sqlx::Result<bool> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM llm_providers WHERE url = $1 AND provider_type = 'llama_server'",
    )
    .bind(url)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}
