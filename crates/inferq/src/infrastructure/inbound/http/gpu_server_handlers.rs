use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::GpuServer;
use crate::infrastructure::outbound::hw_metrics;

use super::state::AppState;

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

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `POST /v1/servers`
pub async fn register_gpu_server(
    State(state): State<AppState>,
    Json(req): Json<RegisterGpuServerRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        )
            .into_response();
    }

    let server = GpuServer {
        id: Uuid::now_v7(),
        name: req.name.trim().to_string(),
        node_exporter_url: req.node_exporter_url.filter(|s| !s.is_empty()),
        registered_at: Utc::now(),
    };

    let id = server.id;
    if let Err(e) = state.gpu_server_registry.register(server).await {
        tracing::error!("failed to register gpu server: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    tracing::info!(%id, name = %req.name, "gpu server registered");
    (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
}

/// `GET /v1/servers`
pub async fn list_gpu_servers(State(state): State<AppState>) -> impl IntoResponse {
    match state.gpu_server_registry.list_all().await {
        Ok(servers) => {
            let summaries: Vec<GpuServerSummary> =
                servers.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(summaries)).into_response()
        }
        Err(e) => {
            tracing::error!("failed to list gpu servers: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

// ── Update ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateGpuServerRequest {
    pub name: Option<String>,
    pub node_exporter_url: Option<String>,
}

/// `PATCH /v1/servers/{id}`
pub async fn update_gpu_server(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGpuServerRequest>,
) -> impl IntoResponse {
    let server = match state.gpu_server_registry.get(id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "server not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "update gpu server: db error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

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

    if let Err(e) = state.gpu_server_registry.update(&updated).await {
        tracing::error!(%id, "failed to update gpu server: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    tracing::info!(%id, name = %updated.name, "gpu server updated");
    (StatusCode::OK, Json(GpuServerSummary::from(updated))).into_response()
}

/// `DELETE /v1/servers/{id}`
pub async fn delete_gpu_server(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.gpu_server_registry.delete(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(%id, "failed to delete gpu server: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `GET /v1/servers/{id}/metrics` — Live hardware metrics from node-exporter.
pub async fn get_server_metrics(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let server = match state.gpu_server_registry.get(id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "server not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "get server metrics: db error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    let Some(ne_url) = server.node_exporter_url.filter(|u| !u.is_empty()) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "no node_exporter_url configured for this server"})),
        )
            .into_response();
    };

    let prev_snapshot = state
        .cpu_snapshot_cache
        .get(&id)
        .map(|r| r.clone());

    match hw_metrics::fetch_node_metrics(&ne_url, prev_snapshot.as_ref()).await {
        Ok((metrics, snapshot)) => {
            state.cpu_snapshot_cache.insert(id, snapshot);
            (StatusCode::OK, Json(metrics)).into_response()
        }
        Err(e) => {
            tracing::warn!(%id, "failed to fetch node metrics from {ne_url}: {e}");
            (StatusCode::OK, Json(hw_metrics::NodeMetrics::default())).into_response()
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
) -> impl IntoResponse {
    let Some(ref repo) = state.analytics_repo else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "analytics not configured"})),
        )
            .into_response();
    };

    let hours = params.hours.unwrap_or(1).max(1).min(1440);

    match repo.server_metrics_history(&id, hours).await {
        Ok(points) => (StatusCode::OK, Json(points)).into_response(),
        Err(e) => {
            tracing::error!(%id, "metrics history failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "query failed"})),
            )
                .into_response()
        }
    }
}
