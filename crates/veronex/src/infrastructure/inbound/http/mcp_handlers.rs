use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::inbound::http::middleware::jwt_auth::{RequireProviderManage, RequireSettingsManage};
use crate::infrastructure::outbound::valkey_keys;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::provider_validation::validate_provider_url;
use super::state::AppState;

// ── Tool discovery helper ──────────────────────────────────────────────────────

/// Fetch tools from MCP server, populate Valkey cache, and persist snapshot to DB.
/// Public so main.rs can call it at startup for pre-existing servers.
pub async fn discover_tools_startup(state: &AppState, server_id: Uuid) {
    discover_and_persist_tools(state, server_id).await;
}

async fn discover_and_persist_tools(state: &AppState, server_id: Uuid) {
    let Some(ref bridge) = state.mcp_bridge else { return };

    let tools = match bridge.session_manager
        .with_session(server_id, |client, session| async move {
            client.list_tools(&session).await
        })
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(%server_id, error = %e, "MCP: tool discovery failed");
            return;
        }
    };

    if tools.is_empty() { return; }

    // Persist snapshot to DB — single batch upsert (avoids N sequential round-trips)
    let tool_names:       Vec<&str>            = tools.iter().map(|t| t.name.as_str()).collect();
    let namespaced_names: Vec<String>          = tools.iter().map(|t| t.namespaced_name()).collect();
    let descriptions:     Vec<&str>            = tools.iter().map(|t| t.description.as_str()).collect();
    let schemas:          Vec<serde_json::Value> = tools.iter()
        .map(|t| serde_json::to_value(&t.input_schema).unwrap_or_default())
        .collect();
    let server_ids: Vec<Uuid> = vec![server_id; tools.len()];

    let _ = sqlx::query(
        "INSERT INTO mcp_server_tools (server_id, tool_name, namespaced_name, description, input_schema, discovered_at)
         SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[], $4::text[], $5::jsonb[], array_fill(now()::timestamptz, ARRAY[$6::int]))
         ON CONFLICT (server_id, tool_name) DO UPDATE
           SET namespaced_name = EXCLUDED.namespaced_name,
               description     = EXCLUDED.description,
               input_schema    = EXCLUDED.input_schema,
               discovered_at   = EXCLUDED.discovered_at"
    )
    .bind(&server_ids as &[Uuid])
    .bind(&tool_names as &[&str])
    .bind(&namespaced_names as &[String])
    .bind(&descriptions as &[&str])
    .bind(&schemas as &[serde_json::Value])
    .bind(tools.len() as i32)
    .execute(&state.pg_pool)
    .await
    .map_err(|e| tracing::warn!(%server_id, count = tools.len(), error = %e, "MCP: batch tool persist failed"));

    // Warm the tool cache from the already-fetched data — avoids a second HTTP call.
    bridge.tool_cache.cache_fetched_tools(server_id, tools.clone()).await;

    tracing::info!(%server_id, count = tools.len(), "MCP: tools discovered and persisted");
}

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
    pub url: Option<String>,
    pub name: Option<String>,
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
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
) -> HandlerResult<Json<Vec<McpServerResponse>>> {
    let rows: Vec<McpServerRow> = sqlx::query_as(
        "SELECT id, name, slug, url, is_enabled, timeout_secs, created_at FROM mcp_servers ORDER BY created_at ASC LIMIT 500"
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
        let hb_keys: Vec<String> = ids.iter().map(|id| valkey_keys::mcp_heartbeat(*id)).collect();
        let liveness: Vec<Option<String>> = match conn.mget(hb_keys).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "MCP: failed to fetch server heartbeats from Valkey");
                vec![]
            }
        };
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
    if name.len() > 128 {
        return Err(AppError::BadRequest("name must be 128 characters or fewer".into()));
    }
    if slug.is_empty()
        || !slug.starts_with(|c: char| c.is_ascii_lowercase())
        || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(AppError::BadRequest("slug must match [a-z][a-z0-9_]*".into()));
    }
    if slug.len() > 64 {
        return Err(AppError::BadRequest("slug must be 64 characters or fewer".into()));
    }
    validate_provider_url(&url)?;

    if let Some(t) = req.timeout_secs {
        if !(1..=300).contains(&t) {
            return Err(AppError::BadRequest("timeout_secs must be between 1 and 300".into()));
        }
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

    // Best-effort connect + tool discovery
    if let Some(ref bridge) = state.mcp_bridge {
        if let Err(e) = bridge.session_manager.connect(id, &slug, &url, timeout_secs as u16).await {
            tracing::warn!(%id, error = %e, "MCP register: session connect failed");
        } else {
            let state_clone = state.clone();
            tokio::spawn(async move {
                discover_and_persist_tools(&state_clone, id).await;
            });
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
    let new_url = req.url.as_deref().unwrap_or(&row.url);
    let new_name = req.name.as_deref().unwrap_or(&row.name);
    let url_changed = req.url.as_deref().is_some_and(|u| u != row.url);

    if req.name.is_some() && new_name.len() > 128 {
        return Err(AppError::BadRequest("name must be 128 characters or fewer".into()));
    }
    if req.url.is_some() {
        validate_provider_url(new_url)?;
    }

    sqlx::query(
        "UPDATE mcp_servers SET is_enabled = $1, url = $2, name = $3, updated_at = now() WHERE id = $4"
    )
        .bind(new_enabled)
        .bind(new_url)
        .bind(new_name)
        .bind(id)
        .execute(&state.pg_pool)
        .await
        .map_err(db_error)?;

    if let Some(ref bridge) = state.mcp_bridge {
        if !new_enabled && row.is_enabled {
            bridge.session_manager.disconnect(id);
            bridge.tool_cache.remove_server(id);
        } else if (new_enabled && !row.is_enabled) || (new_enabled && url_changed) {
            if url_changed {
                bridge.session_manager.disconnect(id);
                bridge.tool_cache.remove_server(id);
            }
            if let Err(e) = bridge.session_manager.connect(id, &row.slug, new_url, row.timeout_secs as u16).await {
                tracing::warn!(%id, error = %e, "MCP patch: session connect failed");
            } else {
                let state_clone = state.clone();
                tokio::spawn(async move {
                    discover_and_persist_tools(&state_clone, id).await;
                });
            }
        }
    }

    emit_audit(&state, &claims, "update", "mcp_server", &id.to_string(), new_name,
        &format!("MCP server '{}' ({}) updated", new_name, id)).await;

    // Liveness + tool count: independent reads, run concurrently.
    let (online, tool_count) = tokio::join!(
        async {
            let Some(ref pool) = state.valkey_pool else { return false };
            let conn: fred::clients::Client = pool.next().clone();
            let key = valkey_keys::mcp_heartbeat(id);
            match conn.get::<Option<String>, _>(key).await {
                Ok(v) => v.is_some(),
                Err(e) => { tracing::warn!(%id, error = %e, "MCP: failed to fetch server heartbeat from Valkey"); false }
            }
        },
        async {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*)::bigint FROM mcp_server_tools WHERE server_id = $1"
            )
            .bind(id)
            .fetch_one(&state.pg_pool)
            .await
            .unwrap_or(0)
        }
    );

    Ok(Json(McpServerResponse {
        id: row.id,
        name: new_name.to_string(),
        slug: row.slug,
        url: new_url.to_string(),
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

// ── Agent discovery (no auth) ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct McpTargetEntry {
    pub id: Uuid,
    pub url: String,
}

/// `GET /v1/mcp/targets` — agent discovery endpoint (no auth required).
///
/// Returns enabled MCP servers as `[{id, url}]` for the agent to health-check.
/// Consumed by veronex-agent on each scrape cycle. No auth — internal network only.
pub async fn list_mcp_targets(State(state): State<AppState>) -> HandlerResult<Json<Vec<McpTargetEntry>>> {
    #[derive(sqlx::FromRow)]
    struct Row { id: Uuid, url: String }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, url FROM mcp_servers WHERE is_enabled = true ORDER BY created_at LIMIT 500"
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(db_error)?;

    Ok(Json(rows.into_iter().map(|r| McpTargetEntry { id: r.id, url: r.url }).collect()))
}
