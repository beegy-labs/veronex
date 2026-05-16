use axum::extract::{Path, Query, State};
use tracing::{instrument, Instrument};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::value_objects::McpId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireMcpManage;
use crate::infrastructure::outbound::persistence::mcp_server_queries as mcp_q;
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

    let _ = mcp_q::upsert_tools_batch(
        &state.pg_pool,
        &server_ids,
        &tool_names,
        &namespaced_names,
        &descriptions,
        &schemas,
        tools.len() as i32,
    )
    .await
    .map_err(|e| tracing::warn!(%server_id, count = tools.len(), error = %e, "MCP: batch tool persist failed"));

    // Update mcp_servers.tool_count + tools_summary (denormalized cache for list API)
    let summary: Vec<serde_json::Value> = tools.iter().map(|t| serde_json::json!({
        "name": t.name,
        "namespaced_name": t.namespaced_name(),
        "description": t.description
    })).collect();
    let summary_json = serde_json::Value::Array(summary);

    let _ = mcp_q::update_tools_summary(&state.pg_pool, server_id, tools.len() as i16, &summary_json)
        .await
        .map_err(|e| tracing::warn!(%server_id, error = %e, "MCP: failed to update tools_summary"));

    // Valkey cache — list API reads from here first (skip DB on hot path)
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let conn: fred::clients::Client = pool.next().clone();
        let key = valkey_keys::mcp_tools_summary(server_id);
        conn.set(&key, summary_json.to_string(), Some(Expiration::EX(crate::domain::constants::MCP_TOOLS_SUMMARY_TTL_SECS)), None, false).await
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
        tokio::spawn(
            async move {
                indexer.index_server_tools(&environment, &tenant_id, server_id, &tools_snap).await;
            }
            .instrument(tracing::info_span!("veronex.mcp_handlers.spawn")),
        );
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

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/mcp/servers`
#[instrument(skip_all)]
pub async fn list_mcp_servers(
    RequireMcpManage(_): RequireMcpManage,
    State(state): State<AppState>,
) -> HandlerResult<Json<Vec<McpServerResponse>>> {
    let rows = mcp_q::list_full(&state.pg_pool).await.map_err(db_error)?;

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

/// `POST /v1/mcp/servers/verify` — probe connectivity to an MCP server URL.
#[instrument(skip_all)]
pub async fn verify_mcp_server(
    _claims: RequireMcpManage,
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    use std::time::Duration;

    let url = match req.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => u.trim().to_string(),
        _ => return AppError::BadRequest("url is required".into()).into_response(),
    };

    if let Err(e) = validate_provider_url(&url) {
        return e.into_response();
    }

    let health_url = format!("{}/health", url.trim_end_matches('/'));
    match state
        .http_client
        .get(&health_url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            (StatusCode::OK, Json(serde_json::json!({"reachable": true}))).into_response()
        }
        Ok(r) => {
            tracing::warn!(url = %url, status = %r.status(), "MCP verify probe returned unexpected status");
            AppError::BadGateway(format!("MCP server returned status {}", r.status())).into_response()
        }
        Err(e) => {
            tracing::warn!(url = %url, error = %e, "MCP verify probe failed");
            AppError::BadGateway("MCP server is not reachable at the given URL".into()).into_response()
        }
    }
}

/// `POST /v1/mcp/servers`
#[instrument(skip_all)]
pub async fn register_mcp_server(
    RequireMcpManage(claims): RequireMcpManage,
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

    mcp_q::insert(&state.pg_pool, id, &name, &slug, &url, timeout_secs)
        .await
        .map_err(db_error)?;

    // Best-effort connect + tool discovery
    if let Some(ref bridge) = state.mcp_bridge {
        if let Err(e) = bridge.session_manager.connect(id, &slug, &url, timeout_secs as u16).await {
            tracing::warn!(%id, error = %e, "MCP register: session connect failed");
        } else {
            let state_clone = state.clone();
            tokio::spawn(
                async move {
                    discover_and_persist_tools(&state_clone, id).await;
                }
                .instrument(tracing::info_span!("veronex.mcp_handlers.spawn")),
            );
        }
    }

    let pub_id = McpId::from_uuid(id);
    emit_audit(&state, &claims, "create", "mcp_server", &pub_id.to_string(), &name,
        &format!("MCP server '{name}' registered (id: {pub_id})")).await;
    tracing::info!(%id, %name, "mcp server registered");

    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": pub_id.to_string()}))))
}

/// `PATCH /v1/mcp/servers/:id`
#[instrument(skip_all)]
pub async fn patch_mcp_server(
    RequireMcpManage(claims): RequireMcpManage,
    State(state): State<AppState>,
    Path(mid): Path<McpId>,
    Json(req): Json<PatchMcpServerRequest>,
) -> HandlerResult<Json<McpServerResponse>> {
    let id = mid.0;
    let row = mcp_q::get_full(&state.pg_pool, id)
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
        if s != row.slug
            && mcp_q::slug_exists_excluding(&state.pg_pool, &s, id)
                .await
                .map_err(db_error)?
        {
            return Err(AppError::Conflict("slug already in use".into()));
        }
        s
    } else {
        row.slug.clone()
    };
    let slug_changed = new_slug != row.slug;

    mcp_q::update_core(&state.pg_pool, id, new_enabled, new_url, new_name, &new_slug)
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
                tokio::spawn(
                    async move {
                        discover_and_persist_tools(&state_clone, id).await;
                    }
                    .instrument(tracing::info_span!("veronex.mcp_handlers.spawn")),
                );
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
            mcp_q::list_tools(&state.pg_pool, id)
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
#[instrument(skip_all)]
pub async fn delete_mcp_server(
    RequireMcpManage(claims): RequireMcpManage,
    State(state): State<AppState>,
    Path(mid): Path<McpId>,
) -> HandlerResult<StatusCode> {
    let id = mid.0;
    let name = mcp_q::get_name(&state.pg_pool, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| AppError::NotFound("mcp server not found".into()))?;

    mcp_q::delete(&state.pg_pool, id).await.map_err(db_error)?;

    if let Some(ref bridge) = state.mcp_bridge {
        bridge.session_manager.disconnect(id);
        bridge.tool_cache.remove_server(id);
    }

    // Remove from Vespa index (non-blocking, non-fatal).
    if let Some(ref indexer) = state.mcp_tool_indexer {
        let indexer = indexer.clone();
        let environment = state.vespa_environment.to_string();
        let tenant_id = state.vespa_tenant_id.to_string();
        tokio::spawn(
            async move {
                indexer.remove_server_tools(&environment, &tenant_id, id).await;
            }
            .instrument(tracing::info_span!("veronex.mcp_handlers.spawn")),
        );
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
#[instrument(skip_all)]
pub async fn list_mcp_targets(State(state): State<AppState>) -> HandlerResult<Json<Vec<McpTargetEntry>>> {
    let rows = mcp_q::list_enabled_for_session(&state.pg_pool).await;
    Ok(Json(rows.into_iter().map(|s| McpTargetEntry { id: s.id.to_string(), url: s.url }).collect()))
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
#[instrument(skip_all)]
pub async fn get_mcp_settings(
    RequireMcpManage(_): RequireMcpManage,
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
#[instrument(skip_all)]
pub async fn patch_mcp_settings(
    RequireMcpManage(claims): RequireMcpManage,
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

#[instrument(skip_all)]

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

    let pg_rows = mcp_q::list_identities(&state.pg_pool).await?;
    let pg_map: std::collections::HashMap<&str, &mcp_q::McpServerIdentity> =
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
