use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::value_objects::{ApiKeyId, McpId};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSettingsManage;
use crate::infrastructure::outbound::valkey_keys;
use super::audit_helpers::emit_audit;
use super::error::AppError;
use super::state::AppState;

/// Invalidate per-key MCP cache entries (ACL + top_k) in Valkey.
/// Called on grant and revoke so the 60-second TTL does not stale-serve.
async fn invalidate_mcp_acl_cache(state: &AppState, key_id: Uuid) {
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        pool.del::<(), _>(&[
            valkey_keys::mcp_key_acl(key_id),
            valkey_keys::mcp_key_top_k(key_id),
        ]).await
            .unwrap_or_else(|e| tracing::warn!(error = %e, "mcp_access: failed to invalidate acl/top_k cache"));
    }
}

#[derive(Serialize)]
pub struct McpAccessEntry {
    pub server_id: McpId,
    pub server_name: String,
    pub slug: String,
    pub is_allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i16>,
}

#[derive(Deserialize)]
pub struct GrantMcpAccessBody {
    pub server_id: McpId,
    pub top_k: Option<i16>,
}

#[derive(sqlx::FromRow)]
struct McpAccessRow {
    id: Uuid,
    name: String,
    slug: String,
    is_allowed: bool,
    top_k: Option<i16>,
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
    Path(kid): Path<ApiKeyId>,
) -> Result<Json<Vec<McpAccessEntry>>, AppError> {
    let key_uuid = kid.0;

    let rows: Vec<McpAccessRow> = sqlx::query_as(
        r#"
        SELECT ms.id, ms.name, ms.slug,
               COALESCE(ka.is_allowed, false) AS is_allowed,
               ka.top_k
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
        server_id: McpId::from_uuid(r.id),
        server_name: r.name,
        slug: r.slug,
        is_allowed: r.is_allowed,
        top_k: r.top_k,
    }).collect()))
}

/// POST /v1/keys/{key_id}/mcp — Grant a key access to an MCP server.
pub async fn grant_key_mcp_access(
    RequireSettingsManage(claims): RequireSettingsManage,
    State(state): State<AppState>,
    Path(kid): Path<ApiKeyId>,
    Json(body): Json<GrantMcpAccessBody>,
) -> Result<impl IntoResponse, AppError> {
    let key_uuid = kid.0;
    let server_uuid = body.server_id.0;

    let server: Option<McpServerRow> = sqlx::query_as(
        "SELECT id, name, slug FROM mcp_servers WHERE id = $1"
    )
    .bind(server_uuid)
    .fetch_optional(&state.pg_pool)
    .await?;

    let server = server.ok_or_else(|| AppError::NotFound("MCP server not found".into()))?;

    sqlx::query(
        r#"
        INSERT INTO mcp_key_access (api_key_id, server_id, is_allowed, top_k)
        VALUES ($1, $2, true, $3)
        ON CONFLICT (api_key_id, server_id) DO UPDATE SET is_allowed = true, top_k = EXCLUDED.top_k
        "#,
    )
    .bind(key_uuid)
    .bind(server_uuid)
    .bind(body.top_k)
    .execute(&state.pg_pool)
    .await?;

    invalidate_mcp_acl_cache(&state, key_uuid).await;

    emit_audit(&state, &claims, "grant", "mcp_key_access",
        &server_uuid.to_string(), &server.name,
        &format!("Granted key {} access to MCP server '{}'", kid, server.name)).await;

    Ok((StatusCode::CREATED, Json(McpAccessEntry {
        server_id: McpId::from_uuid(server.id),
        server_name: server.name,
        slug: server.slug,
        is_allowed: true,
        top_k: body.top_k,
    })))
}

/// DELETE /v1/keys/{key_id}/mcp/{server_id} — Revoke a key's access to an MCP server.
pub async fn revoke_key_mcp_access(
    RequireSettingsManage(claims): RequireSettingsManage,
    State(state): State<AppState>,
    Path((kid, mid)): Path<(ApiKeyId, McpId)>,
) -> Result<StatusCode, AppError> {
    let key_uuid = kid.0;
    let server_uuid = mid.0;

    sqlx::query("DELETE FROM mcp_key_access WHERE api_key_id = $1 AND server_id = $2")
        .bind(key_uuid)
        .bind(server_uuid)
        .execute(&state.pg_pool)
        .await?;

    invalidate_mcp_acl_cache(&state, key_uuid).await;

    emit_audit(&state, &claims, "revoke", "mcp_key_access",
        &server_uuid.to_string(), &server_uuid.to_string(),
        &format!("Revoked key {} access to MCP server {}", kid, mid)).await;

    Ok(StatusCode::NO_CONTENT)
}
