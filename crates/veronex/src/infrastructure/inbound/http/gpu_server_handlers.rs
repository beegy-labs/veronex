use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::GpuServer;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSuper;
use crate::infrastructure::outbound::hw_metrics;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::state::AppState;

type HandlerResult<T> = Result<T, AppError>;

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterGpuServerRequest {
    pub name: String,
    pub node_exporter_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GpuServerSummary {
    pub id: Uuid,
    pub name: String,
    pub node_exporter_url: Option<String>,
    pub registered_at: chrono::DateTime<Utc>,
}

impl From<GpuServer> for GpuServerSummary {
    fn from(s: GpuServer) -> Self {
        Self {
            id: s.id,
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
        .map_err(|e| db_error(e))?
        .ok_or_else(|| AppError::NotFound("server not found".into()))
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `POST /v1/servers`
pub async fn register_gpu_server(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Json(req): Json<RegisterGpuServerRequest>,
) -> HandlerResult<impl IntoResponse> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let server = GpuServer {
        id: Uuid::now_v7(),
        name: req.name.trim().to_string(),
        node_exporter_url: req.node_exporter_url.filter(|s| !s.is_empty()),
        registered_at: Utc::now(),
    };

    let id = server.id;
    state.gpu_server_registry.register(server).await.map_err(|e| db_error(e))?;

    emit_audit(&state, &claims, "create", "gpu_server", &id.to_string(), &req.name,
        &format!("GPU server '{}' registered (id: {})", req.name, id)).await;
    tracing::info!(%id, name = %req.name, "gpu server registered");
    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

/// `GET /v1/servers`
pub async fn list_gpu_servers(State(state): State<AppState>) -> HandlerResult<Json<Vec<GpuServerSummary>>> {
    let servers = state.gpu_server_registry.list_all().await.map_err(|e| db_error(e))?;
    let summaries: Vec<GpuServerSummary> = servers.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

// ── Update ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateGpuServerRequest {
    pub name: Option<String>,
    pub node_exporter_url: Option<String>,
}

/// `PATCH /v1/servers/{id}`
pub async fn update_gpu_server(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGpuServerRequest>,
) -> HandlerResult<Json<GpuServerSummary>> {
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

    state.gpu_server_registry.update(&updated).await.map_err(|e| db_error(e))?;

    emit_audit(&state, &claims, "update", "gpu_server", &id.to_string(), &updated.name,
        &format!("GPU server '{}' ({}) configuration updated", updated.name, id)).await;
    tracing::info!(%id, name = %updated.name, "gpu server updated");
    Ok(Json(GpuServerSummary::from(updated)))
}

/// `DELETE /v1/servers/{id}`
pub async fn delete_gpu_server(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<StatusCode> {
    state.gpu_server_registry.delete(id).await.map_err(|e| db_error(e))?;

    emit_audit(&state, &claims, "delete", "gpu_server", &id.to_string(), &id.to_string(),
        &format!("GPU server {} permanently deleted", id)).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/servers/{id}/metrics` — Live hardware metrics from node-exporter.
pub async fn get_server_metrics(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Json<hw_metrics::NodeMetrics>> {
    let server = get_gpu_server(&state, id).await?;

    let Some(ne_url) = server.node_exporter_url.filter(|u| !u.is_empty()) else {
        return Err(AppError::UnprocessableEntity(
            "no node_exporter_url configured for this server".into(),
        ));
    };

    let prev_snapshot = state
        .cpu_snapshot_cache
        .get(&id)
        .map(|r| r.clone());

    match hw_metrics::fetch_node_metrics(&ne_url, prev_snapshot.as_ref()).await {
        Ok((metrics, snapshot)) => {
            state.cpu_snapshot_cache.insert(id, snapshot);
            Ok(Json(metrics))
        }
        Err(e) => {
            tracing::warn!(%id, "failed to fetch node metrics from {ne_url}: {e}");
            Ok(Json(hw_metrics::NodeMetrics::default()))
        }
    }
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
    Path(id): Path<Uuid>,
    Query(params): Query<MetricsHistoryQuery>,
) -> HandlerResult<impl IntoResponse> {
    let repo = state.analytics_repo.as_ref().ok_or_else(|| {
        AppError::ServiceUnavailable("analytics not configured".into())
    })?;

    let hours = params.hours.unwrap_or(1).clamp(1, 1440);

    let points = repo.server_metrics_history(&id, hours).await.map_err(|e| {
        tracing::error!(%id, "metrics history failed: {e}");
        AppError::Internal(anyhow::anyhow!("query failed"))
    })?;

    Ok(Json(points))
}
