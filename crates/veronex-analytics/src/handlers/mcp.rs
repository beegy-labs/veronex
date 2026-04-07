use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{ch_query_error, validate_hours, HoursQuery};
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct McpServerStatRow {
    pub server_slug: String,
    pub tool_name: String,
    pub total_calls: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub cache_hit_count: u64,
    pub timeout_count: u64,
    pub avg_latency_ms: f64,
}

/// GET /internal/mcp/stats?hours=N — Per-server, per-tool MCP call statistics.
pub async fn get_mcp_stats(
    State(state): State<AppState>,
    Query(params): Query<HoursQuery>,
) -> Result<Json<Vec<McpServerStatRow>>, StatusCode> {
    validate_hours(params.hours)?;
    let hours = params.hours as u32;

    let rows = state
        .ch
        .query(
            "SELECT
                server_slug,
                tool_name,
                sum(call_count)      AS total_calls,
                sum(success_count)   AS success_count,
                sum(error_count)     AS error_count,
                sum(cache_hit_count) AS cache_hit_count,
                sum(timeout_count)   AS timeout_count,
                avgWeighted(avg_latency_ms, call_count) AS avg_latency_ms
             FROM mcp_tool_calls_hourly
             WHERE hour >= now() - INTERVAL ? HOUR
               AND call_count > 0
             GROUP BY server_slug, tool_name
             ORDER BY server_slug ASC, total_calls DESC",
        )
        .bind(hours)
        .fetch_all::<McpServerStatRow>()
        .await
        .map_err(|e| ch_query_error(e, "mcp_stats"))?;

    Ok(Json(rows))
}
