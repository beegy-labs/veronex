use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ch_query_error, format_rfc3339, success_rate, validate_hours, HoursQuery};
use crate::state::AppState;

// ── Aggregate types (match veronex handler shapes) ─────────────────────────────

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct UsageAggregateRow {
    pub request_count: u64,
    pub success_count: u64,
    pub cancelled_count: u64,
    pub error_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct HourlyUsageRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub hour: time::OffsetDateTime,
    pub request_count: u64,
    pub success_count: u64,
    pub cancelled_count: u64,
    pub error_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct HourlyUsageResponse {
    pub hour: String,
    pub request_count: u64,
    pub success_count: u64,
    pub cancelled_count: u64,
    pub error_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct JobUsageRow {
    #[serde(with = "clickhouse::serde::time::datetime64::nanos")]
    pub event_time: time::OffsetDateTime,
    pub request_id: String,
    pub model_name: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
    pub finish_reason: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct JobUsageResponse {
    pub event_time: String,
    pub request_id: String,
    pub model_name: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
    pub finish_reason: String,
    pub status: String,
}

// ── Analytics types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct AnalyticsAggRow {
    pub avg_tps: f64,
    pub avg_prompt_tokens: f64,
    pub avg_completion_tokens: f64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct ModelStatRow {
    pub model_name: String,
    pub request_count: u64,
    pub success_count: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct FinishReasonRow {
    pub reason: String,
    pub count: u64,
}

#[derive(Serialize)]
pub struct ModelStat {
    pub model_name: String,
    pub request_count: u64,
    pub success_count: u64,
    pub success_rate: f64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub avg_latency_ms: f64,
}

#[derive(Serialize)]
pub struct AnalyticsResponse {
    pub avg_tps: f64,
    pub avg_prompt_tokens: f64,
    pub avg_completion_tokens: f64,
    pub models: Vec<ModelStat>,
    pub finish_reasons: Vec<serde_json::Value>,
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /internal/usage?hours=`
pub async fn aggregate_usage(
    State(state): State<AppState>,
    Query(q): Query<HoursQuery>,
) -> Result<Json<UsageAggregateRow>, StatusCode> {
    validate_hours(q.hours)?;

    let result = state
        .ch
        .query(
            "SELECT
                request_count, success_count, cancelled_count, error_count,
                prompt_tokens, completion_tokens,
                prompt_tokens + completion_tokens AS total_tokens
            FROM (
                SELECT
                    count()                                                             AS request_count,
                    countIf(LogAttributes['finish_reason'] = 'stop')                   AS success_count,
                    countIf(LogAttributes['finish_reason'] = 'cancelled')              AS cancelled_count,
                    countIf(LogAttributes['finish_reason'] = 'error')                  AS error_count,
                    sum(toUInt64OrDefault(LogAttributes['prompt_tokens']))             AS prompt_tokens,
                    sum(toUInt64OrDefault(LogAttributes['completion_tokens']))         AS completion_tokens
                FROM otel_logs
                WHERE LogAttributes['event.name'] = 'inference.completed'
                  AND Timestamp >= now() - INTERVAL ? HOUR
            )",
        )
        .bind(q.hours)
        .fetch_one::<UsageAggregateRow>()
        .await
        .map_err(|e| ch_query_error(e, "aggregate_usage query failed"))?;

    Ok(Json(result))
}

/// `GET /internal/usage/{key_id}?hours=`
pub async fn key_usage(
    State(state): State<AppState>,
    Path(key_id): Path<Uuid>,
    Query(q): Query<HoursQuery>,
) -> Result<Json<Vec<HourlyUsageResponse>>, StatusCode> {
    validate_hours(q.hours)?;

    let rows = state
        .ch
        .query(
            "SELECT
                hour, request_count, success_count, cancelled_count, error_count,
                prompt_tokens, completion_tokens,
                prompt_tokens + completion_tokens AS total_tokens
            FROM (
                SELECT
                    toStartOfHour(Timestamp)                                            AS hour,
                    count()                                                             AS request_count,
                    countIf(LogAttributes['finish_reason'] = 'stop')                   AS success_count,
                    countIf(LogAttributes['finish_reason'] = 'cancelled')              AS cancelled_count,
                    countIf(LogAttributes['finish_reason'] = 'error')                  AS error_count,
                    sum(toUInt64OrDefault(LogAttributes['prompt_tokens']))             AS prompt_tokens,
                    sum(toUInt64OrDefault(LogAttributes['completion_tokens']))         AS completion_tokens
                FROM otel_logs
                WHERE LogAttributes['event.name'] = 'inference.completed'
                  AND LogAttributes['api_key_id'] = ?
                  AND Timestamp >= now() - INTERVAL ? HOUR
                GROUP BY hour
                ORDER BY hour ASC
            )",
        )
        .bind(key_id.to_string())
        .bind(q.hours)
        .fetch_all::<HourlyUsageRow>()
        .await
        .map_err(|e| ch_query_error(e, "key_usage query failed"))?;

    let response = rows
        .into_iter()
        .map(|r| HourlyUsageResponse {
            hour: format_rfc3339(r.hour),
            request_count: r.request_count,
            success_count: r.success_count,
            cancelled_count: r.cancelled_count,
            error_count: r.error_count,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
        })
        .collect();

    Ok(Json(response))
}

/// `GET /internal/usage/{key_id}/jobs?hours=`
pub async fn key_usage_jobs(
    State(state): State<AppState>,
    Path(key_id): Path<Uuid>,
    Query(q): Query<HoursQuery>,
) -> Result<Json<Vec<JobUsageResponse>>, StatusCode> {
    validate_hours(q.hours)?;

    let rows = state
        .ch
        .query(
            "SELECT
                Timestamp                                                   AS event_time,
                LogAttributes['request_id']                                 AS request_id,
                LogAttributes['model_name']                                 AS model_name,
                toUInt64OrDefault(LogAttributes['prompt_tokens'])          AS prompt_tokens,
                toUInt64OrDefault(LogAttributes['completion_tokens'])      AS completion_tokens,
                toUInt64OrDefault(LogAttributes['latency_ms'])             AS latency_ms,
                LogAttributes['finish_reason']                              AS finish_reason,
                LogAttributes['status']                                     AS status
            FROM otel_logs
            WHERE LogAttributes['event.name'] = 'inference.completed'
              AND LogAttributes['api_key_id'] = ?
              AND Timestamp >= now() - INTERVAL ? HOUR
            ORDER BY Timestamp DESC
            LIMIT 1000",
        )
        .bind(key_id.to_string())
        .bind(q.hours)
        .fetch_all::<JobUsageRow>()
        .await
        .map_err(|e| ch_query_error(e, "key_usage_jobs query failed"))?;

    let response = rows
        .into_iter()
        .map(|r| JobUsageResponse {
            event_time: format_rfc3339(r.event_time),
            request_id: r.request_id,
            model_name: r.model_name,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            latency_ms: r.latency_ms,
            finish_reason: r.finish_reason,
            status: r.status,
        })
        .collect();

    Ok(Json(response))
}

/// `GET /internal/analytics?hours=`
pub async fn get_analytics(
    State(state): State<AppState>,
    Query(q): Query<HoursQuery>,
) -> Result<Json<AnalyticsResponse>, StatusCode> {
    validate_hours(q.hours)?;

    let agg = state
        .ch
        .query(
            "SELECT
                sum(toUInt64OrDefault(LogAttributes['completion_tokens'])) * 1000.0
                    / greatest(sum(toUInt64OrDefault(LogAttributes['latency_ms'])), 1) AS avg_tps,
                avg(toFloat64OrDefault(LogAttributes['prompt_tokens']))     AS avg_prompt_tokens,
                avg(toFloat64OrDefault(LogAttributes['completion_tokens'])) AS avg_completion_tokens
            FROM otel_logs
            WHERE LogAttributes['event.name'] = 'inference.completed'
              AND LogAttributes['status'] = 'completed'
              AND Timestamp >= now() - INTERVAL ? HOUR",
        )
        .bind(q.hours)
        .fetch_one::<AnalyticsAggRow>()
        .await
        .map_err(|e| ch_query_error(e, "analytics agg query failed"))?;

    let model_rows = state
        .ch
        .query(
            "SELECT
                LogAttributes['model_name']                                         AS model_name,
                count()                                                              AS request_count,
                countIf(LogAttributes['finish_reason'] = 'stop')                   AS success_count,
                sum(toUInt64OrDefault(LogAttributes['prompt_tokens']))             AS total_prompt_tokens,
                sum(toUInt64OrDefault(LogAttributes['completion_tokens']))         AS total_completion_tokens,
                avg(toFloat64OrDefault(LogAttributes['latency_ms']))               AS avg_latency_ms
            FROM otel_logs
            WHERE LogAttributes['event.name'] = 'inference.completed'
              AND Timestamp >= now() - INTERVAL ? HOUR
            GROUP BY model_name
            ORDER BY request_count DESC",
        )
        .bind(q.hours)
        .fetch_all::<ModelStatRow>()
        .await
        .map_err(|e| ch_query_error(e, "analytics model query failed"))?;

    let reason_rows = state
        .ch
        .query(
            "SELECT
                LogAttributes['finish_reason'] AS reason,
                count()                        AS count
            FROM otel_logs
            WHERE LogAttributes['event.name'] = 'inference.completed'
              AND Timestamp >= now() - INTERVAL ? HOUR
            GROUP BY reason
            ORDER BY count DESC",
        )
        .bind(q.hours)
        .fetch_all::<FinishReasonRow>()
        .await
        .map_err(|e| ch_query_error(e, "analytics reason query failed"))?;

    let models = model_rows
        .into_iter()
        .map(|r| ModelStat {
            model_name: r.model_name,
            request_count: r.request_count,
            success_count: r.success_count,
            success_rate: success_rate(r.request_count, r.success_count),
            total_prompt_tokens: r.total_prompt_tokens,
            total_completion_tokens: r.total_completion_tokens,
            avg_latency_ms: r.avg_latency_ms,
        })
        .collect();

    let finish_reasons = reason_rows
        .into_iter()
        .map(|r| serde_json::json!({"reason": r.reason, "count": r.count}))
        .collect();

    Ok(Json(AnalyticsResponse {
        avg_tps: (agg.avg_tps * 10.0).round() / 10.0,
        avg_prompt_tokens: agg.avg_prompt_tokens.round(),
        avg_completion_tokens: agg.avg_completion_tokens.round(),
        models,
        finish_reasons,
    }))
}

#[cfg(test)]
mod tests {
    use crate::handlers::success_rate;

    #[test]
    fn avg_tps_rounding() {
        // Simulates the rounding logic: (val * 10).round() / 10
        let raw = 12.345_f64;
        let rounded = (raw * 10.0).round() / 10.0;
        assert!((rounded - 12.3).abs() < f64::EPSILON);
    }

    #[test]
    fn success_rate_normal() {
        let rate = success_rate(100, 95);
        assert!((rate - 0.95).abs() < f64::EPSILON);
    }
}
