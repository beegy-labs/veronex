use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSettingsManage;
use crate::infrastructure::outbound::valkey_keys;
use super::error::AppError;
use super::state::AppState;

/// Invalidate the per-key MCP ACL cache entry in Valkey.
/// Called on grant and revoke so the 60-second TTL does not stale-serve.
async fn invalidate_mcp_acl_cache(state: &AppState, key_id: Uuid) {
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let _ = pool.del::<(), _>(&valkey_keys::mcp_key_acl(key_id)).await;
    }
}

#[derive(Serialize)]
pub struct McpAccessEntry {
    pub server_id: Uuid,
    pub server_name: String,
    pub slug: String,
    pub is_allowed: bool,
}

#[derive(Deserialize)]
pub struct GrantMcpAccessBody {
    pub server_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct McpAccessRow {
    id: Uuid,
    name: String,
    slug: String,
    is_allowed: bool,
}

#[derive(sqlx::FromRow)]
struct McpServerRow {
    id: Uuid,
    name: String,
    slug: String,
}

/// GET /v1/keys/{key_id}/mcp — List MCP server access for a key.
/// Returns all MCP servers with their allowed status for this key.
pub async fn list_key_mcp_access(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path(key_id): Path<Uuid>,
) -> Result<Json<Vec<McpAccessEntry>>, AppError> {
    let key_uuid = key_id;

    let rows: Vec<McpAccessRow> = sqlx::query_as(
        r#"
        SELECT ms.id, ms.name, ms.slug,
               COALESCE(ka.is_allowed, false) AS is_allowed
        FROM mcp_servers ms
        LEFT JOIN mcp_key_access ka
            ON ka.server_id = ms.id AND ka.api_key_id = $1
        ORDER BY ms.name
        LIMIT 500
        "#,
    )
    .bind(key_uuid)
    .fetch_all(&state.pg_pool)
    .await?;

    Ok(Json(rows.into_iter().map(|r| McpAccessEntry {
        server_id: r.id,
        server_name: r.name,
        slug: r.slug,
        is_allowed: r.is_allowed,
    }).collect()))
}

/// POST /v1/keys/{key_id}/mcp — Grant a key access to an MCP server.
pub async fn grant_key_mcp_access(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path(key_id): Path<Uuid>,
    Json(body): Json<GrantMcpAccessBody>,
) -> Result<impl IntoResponse, AppError> {
    let key_uuid = key_id;
    let server_uuid = body.server_id;

    let server: Option<McpServerRow> = sqlx::query_as(
        "SELECT id, name, slug FROM mcp_servers WHERE id = $1"
    )
    .bind(server_uuid)
    .fetch_optional(&state.pg_pool)
    .await?;

    let server = server.ok_or_else(|| AppError::NotFound("MCP server not found".into()))?;

    sqlx::query(
        r#"
        INSERT INTO mcp_key_access (api_key_id, server_id, is_allowed)
        VALUES ($1, $2, true)
        ON CONFLICT (api_key_id, server_id) DO UPDATE SET is_allowed = true
        "#,
    )
    .bind(key_uuid)
    .bind(server_uuid)
    .execute(&state.pg_pool)
    .await?;

    invalidate_mcp_acl_cache(&state, key_uuid).await;

    Ok((StatusCode::CREATED, Json(McpAccessEntry {
        server_id: server.id,
        server_name: server.name,
        slug: server.slug,
        is_allowed: true,
    })))
}

/// DELETE /v1/keys/{key_id}/mcp/{server_id} — Revoke a key's access to an MCP server.
pub async fn revoke_key_mcp_access(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path((key_id, server_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let key_uuid = key_id;
    let server_uuid = server_id;

    sqlx::query("DELETE FROM mcp_key_access WHERE api_key_id = $1 AND server_id = $2")
        .bind(key_uuid)
        .bind(server_uuid)
        .execute(&state.pg_pool)
        .await?;

    invalidate_mcp_acl_cache(&state, key_uuid).await;

    Ok(StatusCode::NO_CONTENT)
}
