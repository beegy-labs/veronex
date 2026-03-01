use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Deserialize)]
pub struct PerfQuery {
    #[serde(default = "default_hours")]
    pub hours: u32,
}

fn default_hours() -> u32 {
    24
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct LatencyStatsRow {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub total_requests: u64,
    pub success_count: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct HourlyThroughputRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub hour: time::OffsetDateTime,
    pub request_count: u64,
    pub success_count: u64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
}

#[derive(Serialize)]
pub struct HourlyThroughputResponse {
    pub hour: String,
    pub request_count: u64,
    pub success_count: u64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
}

#[derive(Serialize)]
pub struct PerformanceResponse {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub total_requests: u64,
    pub success_rate: f64,
    pub total_tokens: u64,
    pub hourly: Vec<HourlyThroughputResponse>,
}

/// `GET /internal/performance?hours=`
pub async fn get_performance(
    State(state): State<AppState>,
    Query(q): Query<PerfQuery>,
) -> Result<Json<PerformanceResponse>, StatusCode> {
    let stats = state
        .ch
        .query(
            "SELECT
                avgOrDefault(toFloat64OrDefault(LogAttributes['latency_ms']))               AS avg_latency_ms,
                quantileOrDefault(0.50)(toFloat64OrDefault(LogAttributes['latency_ms']))    AS p50_latency_ms,
                quantileOrDefault(0.95)(toFloat64OrDefault(LogAttributes['latency_ms']))    AS p95_latency_ms,
                quantileOrDefault(0.99)(toFloat64OrDefault(LogAttributes['latency_ms']))    AS p99_latency_ms,
                count()                                                                      AS total_requests,
                countIf(LogAttributes['finish_reason'] = 'stop')                            AS success_count,
                sum(toUInt64OrDefault(LogAttributes['completion_tokens']))                  AS total_tokens
            FROM otel_logs
            WHERE LogAttributes['event.name'] = 'inference.completed'
              AND Timestamp >= now() - INTERVAL ? HOUR",
        )
        .bind(q.hours)
        .fetch_one::<LatencyStatsRow>()
        .await
        .map_err(|e| {
            tracing::warn!("performance stats query failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let hourly_rows = state
        .ch
        .query(
            "SELECT
                toStartOfHour(Timestamp)                                                     AS hour,
                count()                                                                      AS request_count,
                countIf(LogAttributes['finish_reason'] = 'stop')                            AS success_count,
                avgOrDefault(toFloat64OrDefault(LogAttributes['latency_ms']))               AS avg_latency_ms,
                sum(toUInt64OrDefault(LogAttributes['completion_tokens']))                  AS total_tokens
            FROM otel_logs
            WHERE LogAttributes['event.name'] = 'inference.completed'
              AND Timestamp >= now() - INTERVAL ? HOUR
            GROUP BY hour
            ORDER BY hour ASC",
        )
        .bind(q.hours)
        .fetch_all::<HourlyThroughputRow>()
        .await
        .map_err(|e| {
            tracing::warn!("performance hourly query failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let success_rate = if stats.total_requests > 0 {
        stats.success_count as f64 / stats.total_requests as f64
    } else {
        0.0
    };

    let hourly = hourly_rows
        .into_iter()
        .map(|r| HourlyThroughputResponse {
            hour: r
                .hour
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            request_count: r.request_count,
            success_count: r.success_count,
            avg_latency_ms: r.avg_latency_ms,
            total_tokens: r.total_tokens,
        })
        .collect();

    Ok(Json(PerformanceResponse {
        avg_latency_ms: stats.avg_latency_ms,
        p50_latency_ms: stats.p50_latency_ms,
        p95_latency_ms: stats.p95_latency_ms,
        p99_latency_ms: stats.p99_latency_ms,
        total_requests: stats.total_requests,
        success_rate,
        total_tokens: stats.total_tokens,
        hourly,
    }))
}
