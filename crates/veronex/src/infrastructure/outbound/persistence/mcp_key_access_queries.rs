//! SSOT for `mcp_key_access` join-table queries (per-key MCP grants).

use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct McpAccessEntryRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub is_allowed: bool,
    pub top_k: Option<i16>,
}

/// Every MCP server, with the requesting key's grant flag/top_k joined
/// in. Servers without a grant row appear with `is_allowed = false`.
pub async fn list_for_key(
    pool: &PgPool,
    key_id: Uuid,
) -> sqlx::Result<Vec<McpAccessEntryRow>> {
    sqlx::query_as::<_, McpAccessEntryRow>(
        "SELECT ms.id, ms.name, ms.slug, \
                COALESCE(ka.is_allowed, false) AS is_allowed, \
                ka.top_k \
         FROM mcp_servers ms \
         LEFT JOIN mcp_key_access ka \
             ON ka.server_id = ms.id AND ka.api_key_id = $1 \
         ORDER BY ms.name LIMIT 500",
    )
    .bind(key_id)
    .fetch_all(pool)
    .await
}

/// Upsert a grant row. `top_k = None` means "no per-key cap".
pub async fn grant(
    pool: &PgPool,
    key_id: Uuid,
    server_id: Uuid,
    top_k: Option<i16>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO mcp_key_access (api_key_id, server_id, is_allowed, top_k) \
         VALUES ($1, $2, true, $3) \
         ON CONFLICT (api_key_id, server_id) DO UPDATE \
           SET is_allowed = true, top_k = EXCLUDED.top_k",
    )
    .bind(key_id)
    .bind(server_id)
    .bind(top_k)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Server ids the key is currently allowed to call. Bounded by an L2
/// cache (Valkey) so this is the cold-path lookup. Returns an empty
/// vector on DB error — the caller treats absence as "no access".
pub async fn list_allowed_server_ids(pool: &PgPool, key_id: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT server_id FROM mcp_key_access WHERE api_key_id = $1 \
         AND is_allowed = true LIMIT 1000",
    )
    .bind(key_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn revoke(pool: &PgPool, key_id: Uuid, server_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM mcp_key_access WHERE api_key_id = $1 AND server_id = $2")
        .bind(key_id)
        .bind(server_id)
        .execute(pool)
        .await
        .map(|_| ())
}
