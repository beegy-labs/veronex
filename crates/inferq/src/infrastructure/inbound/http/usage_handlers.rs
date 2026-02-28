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

// ── Analytics types ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
struct AnalyticsAggRow {
    pub avg_tps: f64,
    pub avg_prompt_tokens: f64,
    pub avg_completion_tokens: f64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
struct ModelStatRow {
    pub model_name: String,
    pub request_count: u64,
    pub success_count: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
struct FinishReasonRow {
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
pub struct FinishReasonStat {
    pub reason: String,
    pub count: u64,
}

#[derive(Serialize)]
pub struct AnalyticsResponse {
    pub avg_tps: f64,
    pub avg_prompt_tokens: f64,
    pub avg_completion_tokens: f64,
    pub models: Vec<ModelStat>,
    pub finish_reasons: Vec<FinishReasonStat>,
}

/// GET /v1/dashboard/analytics — Model distribution, finish reasons, TPS and avg tokens.
pub async fn get_analytics(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<AnalyticsResponse>, StatusCode> {
    let Some(ref client) = state.clickhouse_client else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    // Aggregate TPS + average token counts
    let agg = client
        .query(
            "SELECT
                sum(completion_tokens) * 1000.0 / greatest(sum(latency_ms), 1) AS avg_tps,
                avgOrDefault(prompt_tokens)     AS avg_prompt_tokens,
                avgOrDefault(completion_tokens) AS avg_completion_tokens
            FROM inference_logs
            WHERE event_time >= now() - INTERVAL ? HOUR
              AND status = 'completed'",
        )
        .bind(params.hours)
        .fetch_one::<AnalyticsAggRow>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Per-model statistics
    let model_rows = client
        .query(
            "SELECT
                model_name,
                count()                              AS request_count,
                countIf(finish_reason = 'stop')      AS success_count,
                sum(prompt_tokens)                   AS total_prompt_tokens,
                sum(completion_tokens)               AS total_completion_tokens,
                avgOrDefault(latency_ms)             AS avg_latency_ms
            FROM inference_logs
            WHERE event_time >= now() - INTERVAL ? HOUR
            GROUP BY model_name
            ORDER BY request_count DESC",
        )
        .bind(params.hours)
        .fetch_all::<ModelStatRow>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Finish reason distribution
    let reason_rows = client
        .query(
            "SELECT
                finish_reason AS reason,
                count()       AS count
            FROM inference_logs
            WHERE event_time >= now() - INTERVAL ? HOUR
            GROUP BY finish_reason
            ORDER BY count DESC",
        )
        .bind(params.hours)
        .fetch_all::<FinishReasonRow>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let models = model_rows
        .into_iter()
        .map(|r| {
            let success_rate = if r.request_count > 0 {
                r.success_count as f64 / r.request_count as f64
            } else {
                0.0
            };
            ModelStat {
                model_name: r.model_name,
                request_count: r.request_count,
                success_count: r.success_count,
                success_rate,
                total_prompt_tokens: r.total_prompt_tokens,
                total_completion_tokens: r.total_completion_tokens,
                avg_latency_ms: r.avg_latency_ms,
            }
        })
        .collect();

    let finish_reasons = reason_rows
        .into_iter()
        .map(|r| FinishReasonStat { reason: r.reason, count: r.count })
        .collect();

    Ok(Json(AnalyticsResponse {
        avg_tps: (agg.avg_tps * 10.0).round() / 10.0,
        avg_prompt_tokens: agg.avg_prompt_tokens.round(),
        avg_completion_tokens: agg.avg_completion_tokens.round(),
        models,
        finish_reasons,
    }))
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

// ── Breakdown types ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct BackendBreakdown {
    pub backend: String,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub success_rate: f64,
}

#[derive(Serialize)]
pub struct KeyBreakdown {
    pub key_id: String,
    pub key_name: String,
    pub key_prefix: String,
    pub request_count: i64,
    pub success_count: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub success_rate: f64,
}

#[derive(Serialize)]
pub struct ModelBreakdown {
    pub model_name: String,
    pub backend: String,
    pub request_count: i64,
    pub call_pct: f64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub avg_latency_ms: f64,
}

#[derive(Serialize)]
pub struct UsageBreakdownResponse {
    pub by_backend: Vec<BackendBreakdown>,
    pub by_key: Vec<KeyBreakdown>,
    pub by_model: Vec<ModelBreakdown>,
}

/// GET /v1/usage/breakdown — Backend, API key, and model breakdown from PostgreSQL.
pub async fn usage_breakdown(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageBreakdownResponse>, StatusCode> {
    use sqlx::Row;
    let pool = &state.pg_pool;
    let interval = format!("{} hours", params.hours);

    // ── By backend ────────────────────────────────────────────────────
    let backend_rows = sqlx::query(&format!(
        "SELECT
            backend,
            COUNT(*)                                              AS request_count,
            COUNT(*) FILTER (WHERE status = 'completed')         AS success_count,
            COUNT(*) FILTER (WHERE status = 'failed')            AS error_count,
            COALESCE(SUM(prompt_tokens), 0)                      AS prompt_tokens,
            COALESCE(SUM(completion_tokens), 0)                  AS completion_tokens
         FROM inference_jobs
         WHERE created_at >= now() - interval '{interval}'
         GROUP BY backend
         ORDER BY request_count DESC",
    ))
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let by_backend: Vec<BackendBreakdown> = backend_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            let success_count: i64 = r.try_get("success_count").unwrap_or(0);
            let success_rate = if request_count > 0 {
                (success_count as f64 / request_count as f64 * 1000.0).round() / 10.0
            } else { 0.0 };
            BackendBreakdown {
                backend: r.try_get("backend").unwrap_or_default(),
                request_count,
                success_count,
                error_count: r.try_get("error_count").unwrap_or(0),
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                success_rate,
            }
        })
        .collect();

    // ── By API key ────────────────────────────────────────────────────
    let key_rows = sqlx::query(&format!(
        "SELECT
            k.id::text                                             AS key_id,
            k.name                                                 AS key_name,
            k.key_prefix,
            COUNT(j.id)                                            AS request_count,
            COUNT(j.id) FILTER (WHERE j.status = 'completed')     AS success_count,
            COALESCE(SUM(j.prompt_tokens), 0)                     AS prompt_tokens,
            COALESCE(SUM(j.completion_tokens), 0)                 AS completion_tokens
         FROM inference_jobs j
         JOIN api_keys k ON k.id = j.api_key_id
         WHERE j.created_at >= now() - interval '{interval}'
         GROUP BY k.id, k.name, k.key_prefix
         ORDER BY request_count DESC",
    ))
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let by_key: Vec<KeyBreakdown> = key_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            let success_count: i64 = r.try_get("success_count").unwrap_or(0);
            let success_rate = if request_count > 0 {
                (success_count as f64 / request_count as f64 * 1000.0).round() / 10.0
            } else { 0.0 };
            KeyBreakdown {
                key_id: r.try_get("key_id").unwrap_or_default(),
                key_name: r.try_get("key_name").unwrap_or_default(),
                key_prefix: r.try_get("key_prefix").unwrap_or_default(),
                request_count,
                success_count,
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                success_rate,
            }
        })
        .collect();

    // ── By model + backend ────────────────────────────────────────────
    let total_requests: i64 = by_backend.iter().map(|b| b.request_count).sum();

    let model_rows = sqlx::query(&format!(
        "SELECT
            model_name,
            backend,
            COUNT(*)                                     AS request_count,
            COALESCE(SUM(prompt_tokens), 0)              AS prompt_tokens,
            COALESCE(SUM(completion_tokens), 0)          AS completion_tokens,
            COALESCE(AVG(latency_ms) FILTER (WHERE latency_ms IS NOT NULL), 0) AS avg_latency_ms
         FROM inference_jobs
         WHERE created_at >= now() - interval '{interval}'
         GROUP BY model_name, backend
         ORDER BY request_count DESC",
    ))
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let by_model: Vec<ModelBreakdown> = model_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            let call_pct = if total_requests > 0 {
                (request_count as f64 / total_requests as f64 * 1000.0).round() / 10.0
            } else { 0.0 };
            ModelBreakdown {
                model_name: r.try_get("model_name").unwrap_or_default(),
                backend: r.try_get("backend").unwrap_or_default(),
                request_count,
                call_pct,
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                avg_latency_ms: r.try_get::<f64, _>("avg_latency_ms").unwrap_or(0.0),
            }
        })
        .collect();

    Ok(Json(UsageBreakdownResponse { by_backend, by_key, by_model }))
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
