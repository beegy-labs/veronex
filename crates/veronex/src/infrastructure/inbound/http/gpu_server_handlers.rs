use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::constants::NODE_EXPORTER_TIMEOUT;
use crate::domain::entities::GpuServer;
use crate::domain::value_objects::GpuServerId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::outbound::hw_metrics;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::state::AppState;

type HandlerResult<T> = Result<T, AppError>;

/// Probe a node-exporter URL: returns `Ok(())` if reachable (any HTTP response),
/// `Err` if the connection times out or is refused.
async fn probe_node_exporter(client: &reqwest::Client, url: &str) -> Result<(), String> {
    client
        .get(url)
        .timeout(NODE_EXPORTER_TIMEOUT)
        .send()
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterGpuServerRequest {
    pub name: String,
    pub node_exporter_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyServerRequest {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct GpuServerSummary {
    pub id: GpuServerId,
    pub name: String,
    pub node_exporter_url: Option<String>,
    pub registered_at: chrono::DateTime<Utc>,
}

impl From<GpuServer> for GpuServerSummary {
    fn from(s: GpuServer) -> Self {
        Self {
            id: GpuServerId::from_uuid(s.id),
            name: s.name,
            node_exporter_url: s.node_exporter_url,
            registered_at: s.registered_at,
        }
    }
}

/// Fetch a GPU server by ID or return a structured error.
async fn get_gpu_server(state: &AppState, id: Uuid) -> Result<GpuServer, AppError> {
    state
        .gpu_server_registry
        .get(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| AppError::NotFound("server not found".into()))
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `POST /v1/servers/verify` — validate URL format, duplicate check, and reachability.
pub async fn verify_gpu_server(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<VerifyServerRequest>,
) -> impl IntoResponse {
    let url = req.url.trim().to_string();

    if url.is_empty() {
        return AppError::BadRequest("url is required".into()).into_response();
    }

    // Must be a valid http/https URL.
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return AppError::BadRequest("url must start with http:// or https://".into()).into_response();
    }

    // Duplicate check.
    let count_res: Result<(i64,), _> = sqlx::query_as(
        "SELECT COUNT(*) FROM gpu_servers WHERE node_exporter_url = $1",
    )
    .bind(&url)
    .fetch_one(&state.pg_pool)
    .await;

    match count_res {
        Ok((c,)) if c > 0 => {
            return AppError::Conflict("a server with this URL is already registered".into())
                .into_response();
        }
        Err(e) => return db_error(e).into_response(),
        _ => {}
    }

    // Connectivity check.
    if let Err(e) = probe_node_exporter(&state.http_client, &url).await {
        tracing::warn!(url = %url, error = %e, "node-exporter probe failed");
        return AppError::BadGateway(
            "node-exporter is not reachable at the given URL".into(),
        )
        .into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({"reachable": true}))).into_response()
}

/// `POST /v1/servers`
pub async fn register_gpu_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<RegisterGpuServerRequest>,
) -> HandlerResult<impl IntoResponse> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let node_exporter_url = req.node_exporter_url.as_deref().unwrap_or("").trim().to_string();
    if node_exporter_url.is_empty() {
        return Err(AppError::BadRequest("node_exporter_url is required".into()));
    }

    // Must be a valid http/https URL.
    if !node_exporter_url.starts_with("http://") && !node_exporter_url.starts_with("https://") {
        return Err(AppError::BadRequest("url must start with http:// or https://".into()));
    }

    // Reject duplicate node_exporter_url.
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM gpu_servers WHERE node_exporter_url = $1",
    )
    .bind(&node_exporter_url)
    .fetch_one(&state.pg_pool)
    .await
    .map_err(db_error)?;
    if count > 0 {
        return Err(AppError::Conflict("a server with this URL is already registered".into()));
    }

    // Verify node_exporter is reachable.
    if let Err(e) = probe_node_exporter(&state.http_client, &node_exporter_url).await {
        tracing::warn!(url = %node_exporter_url, error = %e, "node-exporter probe failed on register");
        return Err(AppError::BadGateway(
            "node-exporter is not reachable at the given URL".into(),
        ));
    }

    let server = GpuServer {
        id: Uuid::now_v7(),
        name: req.name.trim().to_string(),
        node_exporter_url: Some(node_exporter_url),
        registered_at: Utc::now(),
    };

    let id = server.id;
    state.gpu_server_registry.register(server).await.map_err(db_error)?;

    let pub_id = GpuServerId::from_uuid(id);
    emit_audit(&state, &claims, "create", "gpu_server", &pub_id.to_string(), &req.name,
        &format!("GPU server '{}' registered (id: {})", req.name, pub_id)).await;
    tracing::info!(%id, name = %req.name, "gpu server registered");
    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": pub_id.to_string()}))))
}

use super::handlers::ListPageParams;

/// `GET /v1/servers`
pub async fn list_gpu_servers(
    State(state): State<AppState>,
    Query(params): Query<ListPageParams>,
) -> HandlerResult<axum::Json<serde_json::Value>> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let offset = (page - 1) * limit;

    let (servers, total) = state.gpu_server_registry.list_page(&search, limit, offset).await.map_err(db_error)?;
    let summaries: Vec<GpuServerSummary> = servers.into_iter().map(Into::into).collect();
    Ok(axum::Json(serde_json::json!({
        "servers": summaries,
        "total": total,
        "page": page,
        "limit": limit,
    })))
}

// ── Update ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateGpuServerRequest {
    pub name: Option<String>,
    pub node_exporter_url: Option<String>,
}

/// `PATCH /v1/servers/{id}`
pub async fn update_gpu_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(gid): Path<GpuServerId>,
    Json(req): Json<UpdateGpuServerRequest>,
) -> HandlerResult<Json<GpuServerSummary>> {
    let id = gid.0;
    let server = get_gpu_server(&state, id).await?;

    let updated = GpuServer {
        id: server.id,
        name: req
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or(server.name),
        node_exporter_url: req
            .node_exporter_url
            .map(|u| u.trim().to_string())
            .map(|u| if u.is_empty() { None } else { Some(u) })
            .unwrap_or(server.node_exporter_url),
        registered_at: server.registered_at,
    };

    state.gpu_server_registry.update(&updated).await.map_err(db_error)?;

    emit_audit(&state, &claims, "update", "gpu_server", &id.to_string(), &updated.name,
        &format!("GPU server '{}' ({}) configuration updated", updated.name, id)).await;
    tracing::info!(%id, name = %updated.name, "gpu server updated");
    Ok(Json(GpuServerSummary::from(updated)))
}

/// `DELETE /v1/servers/{id}`
pub async fn delete_gpu_server(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(gid): Path<GpuServerId>,
) -> HandlerResult<StatusCode> {
    let id = gid.0;
    state.gpu_server_registry.delete(id).await.map_err(db_error)?;

    emit_audit(&state, &claims, "delete", "gpu_server", &gid.to_string(), &gid.to_string(),
        &format!("GPU server {} permanently deleted", gid)).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/servers/{id}/metrics` — Cached hardware metrics from Valkey.
///
/// The health_checker background loop scrapes node-exporter every cycle
/// and caches full `NodeMetrics` per server. This endpoint reads from
/// cache instead of live-scraping, avoiding per-request network calls
/// and scaling to 10K+ providers.
pub async fn get_server_metrics(
    State(state): State<AppState>,
    Path(gid): Path<GpuServerId>,
) -> HandlerResult<Json<hw_metrics::NodeMetrics>> {
    let id = gid.0;
    // Verify server exists
    let _server = get_gpu_server(&state, id).await?;

    let Some(pool) = state.valkey_pool.as_ref() else {
        return Ok(Json(hw_metrics::NodeMetrics::default()));
    };

    match hw_metrics::load_node_metrics(pool, id).await {
        Some(metrics) => Ok(Json(metrics)),
        None => Ok(Json(hw_metrics::NodeMetrics::default())),
    }
}

// ── Batch metrics ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BatchMetricsQuery {
    /// Comma-separated server UUIDs (max 100).
    pub ids: String,
}

/// `GET /v1/servers/metrics/batch?ids=id1,id2,...`
///
/// Returns a map of server_id → NodeMetrics for up to 100 servers in a single
/// request. Replaces N individual `/metrics` calls from the dashboard.
pub async fn get_server_metrics_batch(
    State(state): State<AppState>,
    Query(params): Query<BatchMetricsQuery>,
) -> HandlerResult<Json<std::collections::HashMap<String, hw_metrics::NodeMetrics>>> {
    let Some(pool) = state.valkey_pool.as_ref() else {
        return Ok(Json(std::collections::HashMap::new()));
    };

    let ids: Vec<Uuid> = params
        .ids
        .split(',')
        .take(100)
        .filter_map(|s| {
            let s = s.trim();
            s.parse::<GpuServerId>().map(|id| id.0)
                .or_else(|_| Uuid::parse_str(s))
                .ok()
        })
        .collect();

    let mut result = std::collections::HashMap::with_capacity(ids.len());
    for id in ids {
        let metrics = hw_metrics::load_node_metrics(pool, id)
            .await
            .unwrap_or_default();
        result.insert(GpuServerId::from_uuid(id).to_string(), metrics);
    }

    Ok(Json(result))
}

// ── Metrics history via analytics-service ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MetricsHistoryQuery {
    pub hours: Option<u32>,
}

/// `GET /v1/servers/{id}/metrics/history?hours=N`
///
/// Delegates to the `analytics_repo` (→ veronex-analytics → ClickHouse).
/// Returns 503 when analytics is not configured.
pub async fn get_server_metrics_history(
    State(state): State<AppState>,
    Path(gid): Path<GpuServerId>,
    Query(params): Query<MetricsHistoryQuery>,
) -> HandlerResult<impl IntoResponse> {
    let id = gid.0;
    let repo = state.analytics_repo.as_ref().ok_or_else(|| {
        AppError::ServiceUnavailable("analytics not configured".into())
    })?;

    let hours = params.hours.unwrap_or(1).clamp(1, 1440);

    let points = repo.server_metrics_history(&id, hours).await.map_err(|e| {
        tracing::error!(%id, error = %e, "metrics history failed");
        AppError::ServiceUnavailable("analytics query failed".into())
    })?;

    Ok(Json(points))
}
