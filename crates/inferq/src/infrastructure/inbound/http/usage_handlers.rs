use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::state::AppState;

// ── Query parameters ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UsageQuery {
    #[serde(default = "default_hours")]
    pub hours: u32,
}

fn default_hours() -> u32 {
    24
}

// ── Response types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct UsageAggregate {
    pub request_count: u64,
    pub success_count: u64,
    pub cancelled_count: u64,
    pub error_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct HourlyUsage {
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

#[derive(Serialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct JobUsageRow {
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub event_time: time::OffsetDateTime,
    pub request_id: String,
    pub model_name: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub latency_ms: u32,
    pub finish_reason: String,
    pub status: String,
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/usage — Aggregate usage across all keys.
pub async fn aggregate_usage(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageAggregate>, StatusCode> {
    let Some(ref client) = state.clickhouse_client else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    let result = client
        .query(
            "SELECT
                request_count, success_count, cancelled_count, error_count,
                prompt_tokens, completion_tokens,
                prompt_tokens + completion_tokens AS total_tokens
            FROM (
                SELECT
                    count()                               AS request_count,
                    countIf(finish_reason = 'stop')       AS success_count,
                    countIf(finish_reason = 'cancelled')  AS cancelled_count,
                    countIf(finish_reason = 'error')      AS error_count,
                    sum(prompt_tokens)                    AS prompt_tokens,
                    sum(completion_tokens)                AS completion_tokens
                FROM inference_logs
                WHERE event_time >= now() - INTERVAL ? HOUR
            )",
        )
        .bind(params.hours)
        .fetch_one::<UsageAggregate>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(result))
}

/// GET /v1/usage/{key_id} — Per-key hourly breakdown.
pub async fn key_usage(
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<HourlyUsageResponse>>, StatusCode> {
    let uuid = Uuid::parse_str(&key_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let Some(ref client) = state.clickhouse_client else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    let rows = client
        .query(
            "SELECT
                hour, request_count, success_count, cancelled_count, error_count,
                prompt_tokens, completion_tokens,
                prompt_tokens + completion_tokens AS total_tokens
            FROM (
                SELECT
                    toStartOfHour(event_time)             AS hour,
                    count()                               AS request_count,
                    countIf(finish_reason = 'stop')       AS success_count,
                    countIf(finish_reason = 'cancelled')  AS cancelled_count,
                    countIf(finish_reason = 'error')      AS error_count,
                    sum(prompt_tokens)                    AS prompt_tokens,
                    sum(completion_tokens)                AS completion_tokens
                FROM inference_logs
                WHERE api_key_id = ? AND event_time >= now() - INTERVAL ? HOUR
                GROUP BY hour
                ORDER BY hour ASC
            )",
        )
        .bind(uuid)
        .bind(params.hours)
        .fetch_all::<HourlyUsage>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let response: Vec<HourlyUsageResponse> = rows
        .into_iter()
        .map(|r| {
            let hour_str = r
                .hour
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            HourlyUsageResponse {
                hour: hour_str,
                request_count: r.request_count,
                success_count: r.success_count,
                cancelled_count: r.cancelled_count,
                error_count: r.error_count,
                prompt_tokens: r.prompt_tokens,
                completion_tokens: r.completion_tokens,
                total_tokens: r.total_tokens,
            }
        })
        .collect();

    Ok(Json(response))
}

/// GET /v1/usage/{key_id}/jobs — Individual request list for a key.
pub async fn key_usage_jobs(
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<JobUsageRow>>, StatusCode> {
    let uuid = Uuid::parse_str(&key_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let Some(ref client) = state.clickhouse_client else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    let rows = client
        .query(
            "SELECT
                event_time,
                toString(request_id) AS request_id,
                model_name,
                prompt_tokens,
                completion_tokens,
                latency_ms,
                finish_reason,
                status
            FROM inference_logs
            WHERE api_key_id = ? AND event_time >= now() - INTERVAL ? HOUR
            ORDER BY event_time DESC
            LIMIT 1000",
        )
        .bind(uuid)
        .bind(params.hours)
        .fetch_all::<JobUsageRow>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_query_defaults() {
        let json = serde_json::json!({});
        let query: UsageQuery = serde_json::from_value(json).unwrap();
        assert_eq!(query.hours, 24);
    }

    #[test]
    fn usage_query_custom_hours() {
        let json = serde_json::json!({"hours": 72});
        let query: UsageQuery = serde_json::from_value(json).unwrap();
        assert_eq!(query.hours, 72);
    }

    #[test]
    fn usage_aggregate_serialization() {
        let agg = UsageAggregate {
            request_count: 100,
            success_count: 90,
            cancelled_count: 5,
            error_count: 5,
            prompt_tokens: 10000,
            completion_tokens: 50000,
            total_tokens: 60000,
        };
        let json = serde_json::to_value(&agg).unwrap();
        assert_eq!(json["request_count"], 100);
        assert_eq!(json["total_tokens"], 60000);
    }

    #[test]
    fn hourly_usage_response_serialization() {
        let resp = HourlyUsageResponse {
            hour: "2026-02-22T14:00:00Z".to_string(),
            request_count: 10,
            success_count: 8,
            cancelled_count: 1,
            error_count: 1,
            prompt_tokens: 1000,
            completion_tokens: 5000,
            total_tokens: 6000,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["hour"], "2026-02-22T14:00:00Z");
        assert_eq!(json["request_count"], 10);
    }
}
