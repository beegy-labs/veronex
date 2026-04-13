use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::value_objects::McpId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::{RequireProviderManage, RequireSettingsManage};
use crate::infrastructure::outbound::valkey_keys;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::provider_validation::validate_provider_url;
use super::state::AppState;

// ── Slug validation ────────────────────────────────────────────────────────────

/// Validate MCP server slug: `[a-z][a-z0-9_]*`, max 64 chars.
fn validate_slug(slug: &str) -> Result<(), AppError> {
    if slug.is_empty()
        || !slug.starts_with(|c: char| c.is_ascii_lowercase())
        || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(AppError::BadRequest("slug must match [a-z][a-z0-9_]*".into()));
    }
    if slug.len() > 64 {
        return Err(AppError::BadRequest("slug must be 64 characters or fewer".into()));
    }
    Ok(())
}

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

    // Update mcp_servers.tool_count + tools_summary (denormalized cache for list API)
    let summary: Vec<serde_json::Value> = tools.iter().map(|t| serde_json::json!({
        "name": t.name,
        "namespaced_name": t.namespaced_name(),
        "description": t.description
    })).collect();
    let summary_json = serde_json::Value::Array(summary);

    let _ = sqlx::query(
        "UPDATE mcp_servers SET tool_count = $1, tools_summary = $2, updated_at = now() WHERE id = $3"
    )
    .bind(tools.len() as i16)
    .bind(&summary_json)
    .bind(server_id)
    .execute(&state.pg_pool)
    .await
    .map_err(|e| tracing::warn!(%server_id, error = %e, "MCP: failed to update tools_summary"));

    // Valkey cache — list API reads from here first (skip DB on hot path)
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let conn: fred::clients::Client = pool.next().clone();
        let key = valkey_keys::mcp_tools_summary(server_id);
        conn.set(&key, summary_json.to_string(), Some(Expiration::EX(3600)), None, false).await
            .unwrap_or_else(|e| tracing::warn!(error = %e, %key, "Valkey SET mcp_tools_summary failed"));
    }

    // Warm the tool cache from the already-fetched data — avoids a second HTTP call.
    bridge.tool_cache.cache_fetched_tools(server_id, tools.clone()).await;

    // Index tools into Vespa (non-blocking, non-fatal).
    if let Some(ref indexer) = state.mcp_tool_indexer {
        let indexer = indexer.clone();
        let tools_snap = tools.clone();
        let environment = state.vespa_environment.to_string();
        let tenant_id = state.vespa_tenant_id.to_string();
        tokio::spawn(async move {
            indexer.index_server_tools(&environment, &tenant_id, server_id, &tools_snap).await;
        });
    }

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
    pub slug: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct McpServerResponse {
    id: McpId,
    name: String,
    slug: String,
    url: String,
    is_enabled: bool,
    timeout_secs: i16,
    online: bool,
    tool_count: i64,
    tools: Vec<McpToolSummary>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct McpToolSummary {
    name: String,
    namespaced_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

// ── Row types ──────────────────────────────────────────────────────────────────

struct McpServerRow {
    id: Uuid,
    name: String,
    slug: String,
    url: String,
    is_enabled: bool,
    timeout_secs: i16,
    tool_count: i16,
    tools_summary: serde_json::Value,
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
            tool_count: row.try_get("tool_count")?,
            tools_summary: row.try_get("tools_summary")?,
            created_at: row.try_get("created_at")?,
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
        "SELECT id, name, slug, url, is_enabled, timeout_secs, tool_count, tools_summary, created_at FROM mcp_servers ORDER BY created_at ASC LIMIT 500"
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

    let result = rows
        .into_iter()
        .map(|r| {
            let tools: Vec<McpToolSummary> = r.tools_summary
                .as_array()
                .map(|arr| arr.iter().map(|t| McpToolSummary {
                    name: t["name"].as_str().unwrap_or("").to_string(),
                    namespaced_name: t["namespaced_name"].as_str().unwrap_or("").to_string(),
                    description: t["description"].as_str().map(String::from),
                }).collect())
                .unwrap_or_default();
            McpServerResponse {
                online: online_set.contains(&r.id),
                tool_count: r.tool_count as i64,
                tools,
                id: McpId::from_uuid(r.id),
                name: r.name,
                slug: r.slug,
                url: r.url,
                is_enabled: r.is_enabled,
                timeout_secs: r.timeout_secs,
                created_at: r.created_at,
            }
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
    validate_slug(&slug)?;
    validate_provider_url(&url)?;

    if let Some(t) = req.timeout_secs && !(1..=300).contains(&t) {
        return Err(AppError::BadRequest("timeout_secs must be between 1 and 300".into()));
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

    let pub_id = McpId::from_uuid(id);
    emit_audit(&state, &claims, "create", "mcp_server", &pub_id.to_string(), &name,
        &format!("MCP server '{name}' registered (id: {pub_id})")).await;
    tracing::info!(%id, %name, "mcp server registered");

    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": pub_id.to_string()}))))
}

/// `PATCH /v1/mcp/servers/:id`
pub async fn patch_mcp_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(mid): Path<McpId>,
    Json(req): Json<PatchMcpServerRequest>,
) -> HandlerResult<Json<McpServerResponse>> {
    let id = mid.0;
    let row: McpServerRow = sqlx::query_as(
        "SELECT id, name, slug, url, is_enabled, timeout_secs, tool_count, tools_summary, created_at FROM mcp_servers WHERE id = $1"
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

    let new_slug = if let Some(ref s) = req.slug {
        let s = s.trim().to_string();
        validate_slug(&s)?;
        if s != row.slug {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE slug = $1 AND id != $2)"
            )
            .bind(&s)
            .bind(id)
            .fetch_one(&state.pg_pool)
            .await
            .map_err(db_error)?;
            if exists {
                return Err(AppError::Conflict("slug already in use".into()));
            }
        }
        s
    } else {
        row.slug.clone()
    };
    let slug_changed = new_slug != row.slug;

    sqlx::query(
        "UPDATE mcp_servers SET is_enabled = $1, url = $2, name = $3, slug = $4, updated_at = now() WHERE id = $5"
    )
        .bind(new_enabled)
        .bind(new_url)
        .bind(new_name)
        .bind(&new_slug)
        .bind(id)
        .execute(&state.pg_pool)
        .await
        .map_err(db_error)?;

    if let Some(ref bridge) = state.mcp_bridge {
        if !new_enabled && row.is_enabled {
            bridge.session_manager.disconnect(id);
            bridge.tool_cache.remove_server(id);
        } else if new_enabled && (!row.is_enabled || url_changed || slug_changed) {
            if url_changed || slug_changed {
                bridge.session_manager.disconnect(id);
                bridge.tool_cache.remove_server(id);
            }
            if let Err(e) = bridge.session_manager.connect(id, &new_slug, new_url, row.timeout_secs as u16).await {
                tracing::warn!(%id, error = %e, "MCP patch: session connect failed");
            } else {
                let state_clone = state.clone();
                tokio::spawn(async move {
                    discover_and_persist_tools(&state_clone, id).await;
                });
            }
        }
    }

    emit_audit(&state, &claims, "update", "mcp_server", &mid.to_string(), new_name,
        &format!("MCP server '{}' ({}) updated", new_name, mid)).await;

    // Liveness + tools: independent reads, run concurrently.
    let (online, tools) = tokio::join!(
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
            #[derive(sqlx::FromRow)]
            struct TR { tool_name: String, namespaced_name: String, description: Option<String> }
            sqlx::query_as::<_, TR>(
                "SELECT tool_name, namespaced_name, description FROM mcp_server_tools WHERE server_id = $1 ORDER BY tool_name"
            )
            .bind(id)
            .fetch_all(&state.pg_pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| McpToolSummary { name: r.tool_name, namespaced_name: r.namespaced_name, description: r.description })
            .collect::<Vec<_>>()
        }
    );

    Ok(Json(McpServerResponse {
        id: McpId::from_uuid(row.id),
        name: new_name.to_string(),
        slug: new_slug,
        url: new_url.to_string(),
        is_enabled: new_enabled,
        timeout_secs: row.timeout_secs,
        online,
        tool_count: tools.len() as i64,
        tools,
        created_at: row.created_at,
    }))
}

/// `DELETE /v1/mcp/servers/:id`
pub async fn delete_mcp_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(mid): Path<McpId>,
) -> HandlerResult<StatusCode> {
    let id = mid.0;
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

    // Remove from Vespa index (non-blocking, non-fatal).
    if let Some(ref indexer) = state.mcp_tool_indexer {
        let indexer = indexer.clone();
        let environment = state.vespa_environment.to_string();
        let tenant_id = state.vespa_tenant_id.to_string();
        tokio::spawn(async move {
            indexer.remove_server_tools(&environment, &tenant_id, id).await;
        });
    }

    emit_audit(&state, &claims, "delete", "mcp_server", &mid.to_string(), &name,
        &format!("MCP server '{name}' ({mid}) deleted")).await;
    tracing::info!(%id, %name, "mcp server deleted");

    Ok(StatusCode::NO_CONTENT)
}

// ── Agent discovery (no auth) ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct McpTargetEntry {
    /// Raw UUID string — consumed by veronex-agent for `veronex:mcp:heartbeat:{id}` key.
    pub id: String,
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

    Ok(Json(rows.into_iter().map(|r| McpTargetEntry { id: r.id.to_string(), url: r.url }).collect()))
}

// ── MCP Settings ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct McpSettingsResponse {
    pub routing_cache_ttl_secs: i32,
    pub tool_schema_refresh_secs: i32,
    pub embedding_model: String,
    pub max_tools_per_request: i32,
    pub max_routing_cache_entries: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct PatchMcpSettingsRequest {
    pub routing_cache_ttl_secs: Option<i32>,
    pub tool_schema_refresh_secs: Option<i32>,
    pub embedding_model: Option<String>,
    pub max_tools_per_request: Option<i32>,
    pub max_routing_cache_entries: Option<i32>,
}

/// `GET /v1/mcp/settings`
pub async fn get_mcp_settings(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
) -> HandlerResult<Json<McpSettingsResponse>> {
    let s = state.mcp_settings_repo.get().await.map_err(AppError::Internal)?;
    Ok(Json(McpSettingsResponse {
        routing_cache_ttl_secs: s.routing_cache_ttl_secs,
        tool_schema_refresh_secs: s.tool_schema_refresh_secs,
        embedding_model: s.embedding_model,
        max_tools_per_request: s.max_tools_per_request,
        max_routing_cache_entries: s.max_routing_cache_entries,
        updated_at: s.updated_at,
    }))
}

/// `PATCH /v1/mcp/settings`
pub async fn patch_mcp_settings(
    RequireSettingsManage(claims): RequireSettingsManage,
    State(state): State<AppState>,
    Json(body): Json<PatchMcpSettingsRequest>,
) -> HandlerResult<Json<McpSettingsResponse>> {
    use crate::application::ports::outbound::mcp_settings_repository::McpSettingsUpdate;

    if let Some(v) = body.max_tools_per_request && !(1..=200).contains(&v) {
        return Err(AppError::BadRequest("max_tools_per_request must be 1–200".into()));
    }

    let patch = McpSettingsUpdate {
        routing_cache_ttl_secs: body.routing_cache_ttl_secs,
        tool_schema_refresh_secs: body.tool_schema_refresh_secs,
        embedding_model: body.embedding_model,
        max_tools_per_request: body.max_tools_per_request,
        max_routing_cache_entries: body.max_routing_cache_entries,
    };
    let s = state.mcp_settings_repo.update(patch).await.map_err(AppError::Internal)?;

    emit_audit(&state, &claims, "update", "mcp_settings",
        "mcp_settings", "mcp_settings", "MCP global settings updated").await;

    Ok(Json(McpSettingsResponse {
        routing_cache_ttl_secs: s.routing_cache_ttl_secs,
        tool_schema_refresh_secs: s.tool_schema_refresh_secs,
        embedding_model: s.embedding_model,
        max_tools_per_request: s.max_tools_per_request,
        max_routing_cache_entries: s.max_routing_cache_entries,
        updated_at: s.updated_at,
    }))
}

// ── GET /v1/mcp/stats ─────────────────────────────────────────────────────────

pub async fn get_mcp_stats(
    State(state): State<AppState>,
    Query(params): Query<super::usage_handlers::UsageQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let hours = params.effective_hours()?;
    super::query_helpers::validate_hours(hours)?;

    let slug_stats = if let Some(repo) = state.analytics_repo.as_ref() {
        repo.mcp_server_stats(hours).await.unwrap_or_default()
    } else {
        return Ok(Json(vec![]));
    };

    if slug_stats.is_empty() {
        return Ok(Json(vec![]));
    }

    #[derive(sqlx::FromRow)]
    struct McpRow { id: Uuid, name: String, slug: String }

    let pg_rows: Vec<McpRow> = sqlx::query_as("SELECT id, name, slug FROM mcp_servers ORDER BY name LIMIT 500")
        .fetch_all(&state.pg_pool)
        .await?;

    let pg_map: std::collections::HashMap<&str, &McpRow> =
        pg_rows.iter().map(|r| (r.slug.as_str(), r)).collect();

    let result = slug_stats.into_iter().map(|s| {
        let (server_id, server_name) = pg_map.get(s.server_slug.as_str())
            .map(|r| (r.id.to_string(), r.name.clone()))
            .unwrap_or_else(|| (String::new(), s.server_slug.clone()));

        let success_rate = if s.total_calls > 0 {
            s.success_count as f64 / s.total_calls as f64
        } else { 0.0 };

        serde_json::json!({
            "server_id": server_id,
            "server_name": server_name,
            "server_slug": s.server_slug,
            "total_calls": s.total_calls,
            "success_count": s.success_count,
            "error_count": s.error_count,
            "cache_hit_count": s.cache_hit_count,
            "timeout_count": s.timeout_count,
            "success_rate": success_rate,
            "avg_latency_ms": s.avg_latency_ms,
        })
    }).collect();

    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::validate_slug;

    #[test]
    fn valid_slugs() {
        assert!(validate_slug("abc").is_ok());
        assert!(validate_slug("my_server").is_ok());
        assert!(validate_slug("a1b2c3").is_ok());
        assert!(validate_slug("a").is_ok());
        assert!(validate_slug(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn slug_must_start_with_lowercase() {
        assert!(validate_slug("1abc").is_err());
        assert!(validate_slug("_abc").is_err());
        assert!(validate_slug("Abc").is_err());
    }

    #[test]
    fn slug_disallows_uppercase_and_special_chars() {
        assert!(validate_slug("myServer").is_err());
        assert!(validate_slug("my-server").is_err());
        assert!(validate_slug("my server").is_err());
        assert!(validate_slug("my.server").is_err());
    }

    #[test]
    fn slug_empty_rejected() {
        assert!(validate_slug("").is_err());
    }

    #[test]
    fn slug_max_length_64() {
        assert!(validate_slug(&"a".repeat(64)).is_ok());
        assert!(validate_slug(&"a".repeat(65)).is_err());
    }
}
