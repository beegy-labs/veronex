//! Ad-hoc SSOT queries for `api_keys` that don't fit the
//! `ApiKeyRepository` port.

use sqlx::PgPool;
use uuid::Uuid;

/// Update the per-key MCP cap budget. The handler clears the Valkey
/// cap-points cache after this returns.
pub async fn update_mcp_cap_points(
    pool: &PgPool,
    key_id: Uuid,
    cap_points: i16,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE api_keys SET mcp_cap_points = $1 WHERE id = $2")
        .bind(cap_points)
        .bind(key_id)
        .execute(pool)
        .await
        .map(|_| ())
}
