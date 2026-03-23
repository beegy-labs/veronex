use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::state::AppState;

type HandlerResult<T> = Result<T, AppError>;

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterMcpServerRequest {
    pub name: String,
    pub slug: String,
    pub url: String,
    pub timeout_secs: Option<i16>,
}

#[derive(Debug, Deserialize)]
pub struct PatchMcpServerRequest {
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct McpServerResponse {
    id: Uuid,
    name: String,
    slug: String,
    url: String,
    is_enabled: bool,
    timeout_secs: i16,
    online: bool,
    tool_count: i64,
    created_at: DateTime<Utc>,
}

// ── Row types ──────────────────────────────────────────────────────────────────

struct McpServerRow {
    id: Uuid,
    name: String,
    slug: String,
    url: String,
    is_enabled: bool,
    timeout_secs: i16,
    created_at: DateTime<Utc>,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for McpServerRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> sqlx::Result<Self> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            slug: row.try_get("slug")?,
            url: row.try_get("url")?,
            is_enabled: row.try_get("is_enabled")?,
            timeout_secs: row.try_get("timeout_secs")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

struct ToolCountRow {
    server_id: Uuid,
    cnt: i64,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for ToolCountRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> sqlx::Result<Self> {
        use sqlx::Row;
        Ok(Self {
            server_id: row.try_get("server_id")?,
            cnt: row.try_get("cnt")?,
        })
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/mcp/servers`
pub async fn list_mcp_servers(
    State(state): State<AppState>,
) -> HandlerResult<Json<Vec<McpServerResponse>>> {
    let rows: Vec<McpServerRow> = sqlx::query_as(
        "SELECT id, name, slug, url, is_enabled, timeout_secs, created_at FROM mcp_servers ORDER BY created_at ASC"
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(db_error)?;

    if rows.is_empty() {
        return Ok(Json(vec![]));
    }

    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();

    // Batch-check Valkey heartbeats
    let online_set: std::collections::HashSet<Uuid> = if let Some(ref pool) = state.valkey_pool {
        let conn: fred::clients::Client = pool.next().clone();
        let hb_keys: Vec<String> = ids.iter().map(|id| format!("veronex:mcp:heartbeat:{id}")).collect();
        let liveness: Vec<Option<String>> = conn.mget(hb_keys).await.unwrap_or_default();
        ids.iter()
            .zip(liveness.into_iter())
            .filter_map(|(id, v)| if v.is_some() { Some(*id) } else { None })
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    // Batch-count tools per server
    let tool_rows: Vec<ToolCountRow> = sqlx::query_as(
        "SELECT server_id, COUNT(*)::bigint AS cnt FROM mcp_server_tools WHERE server_id = ANY($1) GROUP BY server_id"
    )
    .bind(&ids as &[Uuid])
    .fetch_all(&state.pg_pool)
    .await
    .map_err(db_error)?;

    let mut tool_counts: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();
    for row in tool_rows {
        tool_counts.insert(row.server_id, row.cnt);
    }

    let result = rows
        .into_iter()
        .map(|r| McpServerResponse {
            online: online_set.contains(&r.id),
            tool_count: tool_counts.get(&r.id).copied().unwrap_or(0),
            id: r.id,
            name: r.name,
            slug: r.slug,
            url: r.url,
            is_enabled: r.is_enabled,
            timeout_secs: r.timeout_secs,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(result))
}

/// `POST /v1/mcp/servers`
pub async fn register_mcp_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<RegisterMcpServerRequest>,
) -> HandlerResult<impl IntoResponse> {
    let name = req.name.trim().to_string();
    let slug = req.slug.trim().to_string();
    let url = req.url.trim().to_string();

    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(AppError::BadRequest("slug must match [a-z0-9_]+".into()));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AppError::BadRequest("url must start with http:// or https://".into()));
    }

    let id = Uuid::now_v7();
    let timeout_secs = req.timeout_secs.unwrap_or(30);

    sqlx::query(
        "INSERT INTO mcp_servers (id, name, slug, url, timeout_secs) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(id)
    .bind(&name)
    .bind(&slug)
    .bind(&url)
    .bind(timeout_secs)
    .execute(&state.pg_pool)
    .await
    .map_err(db_error)?;

    // Best-effort connect
    if let Some(ref bridge) = state.mcp_bridge {
        if let Err(e) = bridge.session_manager.connect(id, &slug, &url).await {
            tracing::warn!(%id, error = %e, "MCP register: session connect failed");
        }
    }

    emit_audit(&state, &claims, "create", "mcp_server", &id.to_string(), &name,
        &format!("MCP server '{name}' registered (id: {id})")).await;
    tracing::info!(%id, %name, "mcp server registered");

    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

/// `PATCH /v1/mcp/servers/:id`
pub async fn patch_mcp_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchMcpServerRequest>,
) -> HandlerResult<Json<McpServerResponse>> {
    let row: McpServerRow = sqlx::query_as(
        "SELECT id, name, slug, url, is_enabled, timeout_secs, created_at FROM mcp_servers WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(db_error)?
    .ok_or_else(|| AppError::NotFound("mcp server not found".into()))?;

    let new_enabled = req.is_enabled.unwrap_or(row.is_enabled);

    sqlx::query("UPDATE mcp_servers SET is_enabled = $1, updated_at = now() WHERE id = $2")
        .bind(new_enabled)
        .bind(id)
        .execute(&state.pg_pool)
        .await
        .map_err(db_error)?;

    if let Some(ref bridge) = state.mcp_bridge {
        if !new_enabled && row.is_enabled {
            bridge.session_manager.disconnect(id);
            bridge.tool_cache.remove_server(id);
        } else if new_enabled && !row.is_enabled {
            if let Err(e) = bridge.session_manager.connect(id, &row.slug, &row.url).await {
                tracing::warn!(%id, error = %e, "MCP patch: session connect failed");
            }
        }
    }

    emit_audit(&state, &claims, "update", "mcp_server", &id.to_string(), &row.name,
        &format!("MCP server '{}' ({}) updated is_enabled={}", row.name, id, new_enabled)).await;

    // Liveness check for single server
    let online = if let Some(ref pool) = state.valkey_pool {
        let conn: fred::clients::Client = pool.next().clone();
        let key = format!("veronex:mcp:heartbeat:{id}");
        let v: Option<String> = conn.get(key).await.unwrap_or(None);
        v.is_some()
    } else {
        false
    };

    let tool_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM mcp_server_tools WHERE server_id = $1"
    )
    .bind(id)
    .fetch_one(&state.pg_pool)
    .await
    .map_err(db_error)?;

    Ok(Json(McpServerResponse {
        id: row.id,
        name: row.name,
        slug: row.slug,
        url: row.url,
        is_enabled: new_enabled,
        timeout_secs: row.timeout_secs,
        online,
        tool_count,
        created_at: row.created_at,
    }))
}

/// `DELETE /v1/mcp/servers/:id`
pub async fn delete_mcp_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<StatusCode> {
    let row: Option<(String,)> = sqlx::query_as("SELECT name FROM mcp_servers WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pg_pool)
        .await
        .map_err(db_error)?;
    let (name,) = row.ok_or_else(|| AppError::NotFound("mcp server not found".into()))?;

    sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
        .bind(id)
        .execute(&state.pg_pool)
        .await
        .map_err(db_error)?;

    if let Some(ref bridge) = state.mcp_bridge {
        bridge.session_manager.disconnect(id);
        bridge.tool_cache.remove_server(id);
    }

    emit_audit(&state, &claims, "delete", "mcp_server", &id.to_string(), &name,
        &format!("MCP server '{name}' ({id}) deleted")).await;
    tracing::info!(%id, %name, "mcp server deleted");

    Ok(StatusCode::NO_CONTENT)
}
