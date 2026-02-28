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
    /// node-exporter endpoint, e.g. `"http://192.168.1.10:9100"`.
    /// Leave empty to register a server without metric collection.
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
        name: req.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or(server.name),
        node_exporter_url: req.node_exporter_url.map(|u| u.trim().to_string()).map(|u| if u.is_empty() { None } else { Some(u) }).unwrap_or(server.node_exporter_url),
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

/// `GET /v1/servers/{id}/metrics`
///
/// Fetches live hardware metrics directly from the server's node-exporter
/// endpoint.  Returns `scrape_ok: false` when the endpoint is unreachable
/// instead of a server error, so the UI can show a connectivity status.
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
        .lock()
        .unwrap()
        .get(&id)
        .cloned();

    match hw_metrics::fetch_node_metrics(&ne_url, prev_snapshot.as_ref()).await {
        Ok((metrics, snapshot)) => {
            state.cpu_snapshot_cache.lock().unwrap().insert(id, snapshot);
            (StatusCode::OK, Json(metrics)).into_response()
        }
        Err(e) => {
            tracing::warn!(%id, "failed to fetch node metrics from {ne_url}: {e}");
            (StatusCode::OK, Json(hw_metrics::NodeMetrics::default())).into_response()
        }
    }
}

// ── ClickHouse metrics history ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MetricsHistoryQuery {
    pub hours: Option<u32>,
}

/// Internal ClickHouse row: chip label lookup.
#[derive(Debug, Deserialize, clickhouse::Row)]
struct ChipRow {
    chip: String,
}

/// Internal ClickHouse row: one 1-minute bucket of hardware metrics.
#[derive(Debug, Deserialize, clickhouse::Row)]
struct ServerMetricsHistoryRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    ts: time::OffsetDateTime,
    mem_total_mb: f64,
    mem_avail_mb: f64,
    /// 0.0 when no GPU temp data exists for this bucket.
    gpu_temp_c: f64,
    /// 0.0 when no GPU power data exists for this bucket.
    gpu_power_w: f64,
}

/// JSON response: one time-series point per minute.
#[derive(Debug, Serialize)]
pub struct ServerMetricsPoint {
    /// ISO 8601 timestamp (start of 1-minute bucket).
    pub ts: String,
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,
    pub gpu_power_w: Option<f64>,
}

/// `GET /v1/servers/{id}/metrics/history?hours=N`
///
/// Returns 1-minute bucketed hardware metrics from ClickHouse for the given
/// server. `hours` defaults to 1 (max 168 = 1 week).
/// Returns 503 when ClickHouse is not configured.
pub async fn get_server_metrics_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<MetricsHistoryQuery>,
) -> impl IntoResponse {
    let Some(ref client) = state.clickhouse_client else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "ClickHouse not configured"})),
        )
            .into_response();
    };

    let hours = params.hours.unwrap_or(1).max(1).min(168);
    let server_id = id.to_string();

    // Step 1: find the amdgpu hwmon chip label for this server so we can
    // filter temperature and power rows correctly.
    let chip_rows = client
        .query(
            "SELECT DISTINCT Attributes['chip'] AS chip
             FROM otel_metrics_gauge
             WHERE MetricName = 'node_hwmon_chip_names'
               AND Attributes['chip_name'] = 'amdgpu'
               AND Attributes['server_id'] = ?
             LIMIT 1",
        )
        .bind(&server_id)
        .fetch_all::<ChipRow>()
        .await
        .unwrap_or_default();

    let gpu_chip = chip_rows
        .into_iter()
        .next()
        .map(|r| r.chip)
        .unwrap_or_default();

    // Step 2: 1-minute pivot query.
    // avgIf returns 0.0 when no rows match the condition; we convert to None below.
    let rows = match client
        .query(
            "SELECT
                toStartOfInterval(TimeUnix, INTERVAL 1 MINUTE) AS ts,
                toFloat64(maxIf(Value, MetricName = 'node_memory_MemTotal_bytes') / 1048576.0) AS mem_total_mb,
                toFloat64(avgIf(Value, MetricName = 'node_memory_MemAvailable_bytes') / 1048576.0) AS mem_avail_mb,
                avgIf(Value,
                    MetricName = 'node_hwmon_temp_celsius'
                    AND Attributes['chip'] = ?
                    AND Attributes['sensor'] = 'temp1') AS gpu_temp_c,
                avgIf(Value,
                    MetricName IN ('node_hwmon_power_average_watt', 'node_hwmon_power_average_watts')
                    AND Attributes['chip'] = ?) AS gpu_power_w
            FROM otel_metrics_gauge
            WHERE Attributes['server_id'] = ?
              AND TimeUnix >= now() - INTERVAL ? HOUR
            GROUP BY ts
            ORDER BY ts",
        )
        .bind(&gpu_chip)
        .bind(&gpu_chip)
        .bind(&server_id)
        .bind(hours)
        .fetch_all::<ServerMetricsHistoryRow>()
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(%id, "metrics history query failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "query failed"})),
            )
                .into_response();
        }
    };

    let points: Vec<ServerMetricsPoint> = rows
        .into_iter()
        .map(|r| ServerMetricsPoint {
            ts: r
                .ts
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            mem_total_mb: r.mem_total_mb as u64,
            mem_avail_mb: r.mem_avail_mb as u64,
            gpu_temp_c: if r.gpu_temp_c > 0.0 { Some(r.gpu_temp_c) } else { None },
            gpu_power_w: if r.gpu_power_w > 0.0 { Some(r.gpu_power_w) } else { None },
        })
        .collect();

    (StatusCode::OK, Json(points)).into_response()
}
