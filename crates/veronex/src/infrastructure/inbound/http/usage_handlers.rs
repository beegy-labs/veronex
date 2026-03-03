use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::analytics_repository::{
    AnalyticsSummary, AuditFilters, HourlyUsage, UsageAggregate, UsageJob,
};

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

// ── Response types (re-use port types directly) ────────────────────

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

impl From<HourlyUsage> for HourlyUsageResponse {
    fn from(h: HourlyUsage) -> Self {
        Self {
            hour: h.hour,
            request_count: h.request_count,
            success_count: h.success_count,
            cancelled_count: h.cancelled_count,
            error_count: h.error_count,
            prompt_tokens: h.prompt_tokens,
            completion_tokens: h.completion_tokens,
            total_tokens: h.total_tokens,
        }
    }
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/usage — Aggregate usage across all keys.
pub async fn aggregate_usage(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageAggregate>, StatusCode> {
    let repo = state
        .analytics_repo
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let result = repo
        .aggregate_usage(params.hours)
        .await
        .map_err(|e| {
            tracing::warn!("aggregate_usage failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(result))
}

/// GET /v1/usage/{key_id} — Per-key hourly breakdown.
pub async fn key_usage(
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<HourlyUsageResponse>>, StatusCode> {
    let uuid = Uuid::parse_str(&key_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let repo = state
        .analytics_repo
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let rows = repo
        .key_usage_hourly(&uuid, params.hours)
        .await
        .map_err(|e| {
            tracing::warn!("key_usage failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// GET /v1/dashboard/analytics — Model distribution, finish reasons, TPS and avg tokens.
pub async fn get_analytics(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<AnalyticsSummary>, StatusCode> {
    let repo = state
        .analytics_repo
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let summary = repo
        .analytics_summary(params.hours)
        .await
        .map_err(|e| {
            tracing::warn!("analytics_summary failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(summary))
}

/// GET /v1/usage/{key_id}/jobs — Individual request list for a key.
pub async fn key_usage_jobs(
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<UsageJob>>, StatusCode> {
    let uuid = Uuid::parse_str(&key_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let repo = state
        .analytics_repo
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let jobs = repo
        .key_usage_jobs(&uuid, params.hours)
        .await
        .map_err(|e| {
            tracing::warn!("key_usage_jobs failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(jobs))
}

/// GET /v1/usage/{key_id}/models — Per-key model breakdown from PostgreSQL.
/// Returns which models the key has used, with request counts and token stats.
pub async fn key_model_breakdown(
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<ModelBreakdown>>, StatusCode> {
    use sqlx::Row;

    let uuid = Uuid::parse_str(&key_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let interval = format!("{} hours", params.hours);
    let pool = &state.pg_pool;

    let total_row = sqlx::query(&format!(
        "SELECT COUNT(*) AS total FROM inference_jobs
         WHERE api_key_id = $1
           AND created_at > NOW() - INTERVAL '{interval}'"
    ))
    .bind(uuid)
    .fetch_one(pool)
    .await
    .map_err(|e| { tracing::warn!("key_model_breakdown total: {e}"); StatusCode::INTERNAL_SERVER_ERROR })?;
    let total: i64 = total_row.try_get("total").unwrap_or(1).max(1);

    let rows = sqlx::query(&format!(
        "SELECT
            j.model_name,
            j.provider_type,
            COUNT(*)                                                                           AS request_count,
            COUNT(*) FILTER (WHERE j.status = 'completed')                                    AS success_count,
            COALESCE(SUM(j.prompt_tokens), 0)                                                  AS prompt_tokens,
            COALESCE(SUM(j.completion_tokens), 0)                                              AS completion_tokens,
            COALESCE(AVG(j.latency_ms) FILTER (WHERE j.status = 'completed' AND j.latency_ms > 0), 0) AS avg_latency_ms,
            CASE
                WHEN j.provider_type = 'ollama' THEN 0.0
                WHEN pricing.input_per_1m IS NOT NULL THEN
                    (COALESCE(SUM(j.prompt_tokens), 0)::float8     / 1000000.0 * pricing.input_per_1m) +
                    (COALESCE(SUM(j.completion_tokens), 0)::float8 / 1000000.0 * pricing.output_per_1m)
                ELSE NULL
            END AS estimated_cost_usd
         FROM inference_jobs j
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m FROM model_pricing
             WHERE provider = j.provider_type
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
         WHERE j.api_key_id = $1
           AND j.created_at > NOW() - INTERVAL '{interval}'
         GROUP BY j.model_name, j.provider_type, pricing.input_per_1m, pricing.output_per_1m
         ORDER BY request_count DESC
         LIMIT 50"
    ))
    .bind(uuid)
    .fetch_all(pool)
    .await
    .map_err(|e| { tracing::warn!("key_model_breakdown: {e}"); StatusCode::INTERNAL_SERVER_ERROR })?;

    let breakdown: Vec<ModelBreakdown> = rows.iter().map(|r| {
        let request_count: i64 = r.try_get("request_count").unwrap_or(0);
        ModelBreakdown {
            model_name:        r.try_get("model_name").unwrap_or_default(),
            provider_type:     r.try_get("provider_type").unwrap_or_default(),
            request_count,
            call_pct:          request_count as f64 / total as f64 * 100.0,
            prompt_tokens:     r.try_get("prompt_tokens").unwrap_or(0),
            completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
            avg_latency_ms:    r.try_get("avg_latency_ms").unwrap_or(0.0),
            estimated_cost_usd: r.try_get("estimated_cost_usd").unwrap_or(None),
        }
    }).collect();

    Ok(Json(breakdown))
}

// ── Breakdown types (still queried from PG directly) ──────────────────────────

#[derive(Serialize)]
pub struct ProviderBreakdown {
    pub provider_type: String,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub success_rate: f64,
    /// Estimated API cost (USD). $0.00 for Ollama. None = no pricing configured.
    pub estimated_cost_usd: Option<f64>,
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
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Serialize)]
pub struct ModelBreakdown {
    pub model_name: String,
    pub provider_type: String,
    pub request_count: i64,
    pub call_pct: f64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub avg_latency_ms: f64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Serialize)]
pub struct UsageBreakdownResponse {
    pub by_providers: Vec<ProviderBreakdown>,
    pub by_key: Vec<KeyBreakdown>,
    pub by_model: Vec<ModelBreakdown>,
    /// Sum of estimated costs across all providers (USD).
    pub total_cost_usd: f64,
}

/// GET /v1/usage/breakdown — Backend, API key, and model breakdown from PostgreSQL.
pub async fn usage_breakdown(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageBreakdownResponse>, StatusCode> {
    use sqlx::Row;
    let pool = &state.pg_pool;
    let interval = format!("{} hours", params.hours);

    // ── By provider (with LATERAL pricing join) ────────────────────────
    let provider_rows = sqlx::query(&format!(
        "SELECT
            j.provider_type,
            COUNT(*)                                              AS request_count,
            COUNT(*) FILTER (WHERE j.status = 'completed')       AS success_count,
            COUNT(*) FILTER (WHERE j.status = 'failed')          AS error_count,
            COALESCE(SUM(j.prompt_tokens), 0)                    AS prompt_tokens,
            COALESCE(SUM(j.completion_tokens), 0)                AS completion_tokens,
            CASE
                WHEN j.provider_type = 'ollama' THEN 0.0
                WHEN pricing.input_per_1m IS NOT NULL THEN
                    (COALESCE(SUM(j.prompt_tokens), 0)::float8 / 1000000.0 * pricing.input_per_1m) +
                    (COALESCE(SUM(j.completion_tokens), 0)::float8 / 1000000.0 * pricing.output_per_1m)
                ELSE NULL
            END AS estimated_cost_usd
         FROM inference_jobs j
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m
             FROM model_pricing
             WHERE provider = j.provider_type AND model_name = '*'
             LIMIT 1
         ) pricing ON true
         WHERE j.created_at >= now() - interval '{interval}'
         GROUP BY j.provider_type, j.provider_type, pricing.input_per_1m, pricing.output_per_1m
         ORDER BY request_count DESC",
    ))
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let by_providers: Vec<ProviderBreakdown> = provider_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            let success_count: i64 = r.try_get("success_count").unwrap_or(0);
            let success_rate = if request_count > 0 {
                (success_count as f64 / request_count as f64 * 1000.0).round() / 10.0
            } else {
                0.0
            };
            ProviderBreakdown {
                provider_type: r.try_get("provider_type").unwrap_or_default(),
                request_count,
                success_count,
                error_count: r.try_get("error_count").unwrap_or(0),
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                success_rate,
                estimated_cost_usd: r.try_get("estimated_cost_usd").unwrap_or(None),
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
            COALESCE(SUM(j.completion_tokens), 0)                 AS completion_tokens,
            SUM(
                CASE
                    WHEN j.provider_type = 'ollama' THEN 0.0
                    WHEN j.prompt_tokens IS NOT NULL AND j.completion_tokens IS NOT NULL THEN
                        (j.prompt_tokens::float8 / 1000000.0 * COALESCE(pricing.input_per_1m, 0)) +
                        (j.completion_tokens::float8 / 1000000.0 * COALESCE(pricing.output_per_1m, 0))
                    ELSE NULL
                END
            ) AS estimated_cost_usd
         FROM inference_jobs j
         JOIN api_keys k ON k.id = j.api_key_id
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m
             FROM model_pricing
             WHERE provider = j.provider_type
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
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
            } else {
                0.0
            };
            KeyBreakdown {
                key_id: r.try_get("key_id").unwrap_or_default(),
                key_name: r.try_get("key_name").unwrap_or_default(),
                key_prefix: r.try_get("key_prefix").unwrap_or_default(),
                request_count,
                success_count,
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                success_rate,
                estimated_cost_usd: r.try_get("estimated_cost_usd").unwrap_or(None),
            }
        })
        .collect();

    // ── By model + provider ────────────────────────────────────────────
    let total_requests: i64 = by_providers.iter().map(|b| b.request_count).sum();

    let model_rows = sqlx::query(&format!(
        "SELECT
            j.model_name,
            j.provider_type,
            COUNT(*)                                     AS request_count,
            COALESCE(SUM(j.prompt_tokens), 0)            AS prompt_tokens,
            COALESCE(SUM(j.completion_tokens), 0)        AS completion_tokens,
            COALESCE(AVG(j.latency_ms) FILTER (WHERE j.latency_ms IS NOT NULL), 0) AS avg_latency_ms,
            SUM(
                CASE
                    WHEN j.provider_type = 'ollama' THEN 0.0
                    WHEN j.prompt_tokens IS NOT NULL AND j.completion_tokens IS NOT NULL THEN
                        (j.prompt_tokens::float8 / 1000000.0 * COALESCE(pricing.input_per_1m, 0)) +
                        (j.completion_tokens::float8 / 1000000.0 * COALESCE(pricing.output_per_1m, 0))
                    ELSE NULL
                END
            ) AS estimated_cost_usd
         FROM inference_jobs j
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m
             FROM model_pricing
             WHERE provider = j.provider_type
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
         WHERE j.created_at >= now() - interval '{interval}'
         GROUP BY j.model_name, j.provider_type
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
            } else {
                0.0
            };
            ModelBreakdown {
                model_name: r.try_get("model_name").unwrap_or_default(),
                provider_type: r.try_get("provider_type").unwrap_or_default(),
                request_count,
                call_pct,
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                avg_latency_ms: r.try_get::<f64, _>("avg_latency_ms").unwrap_or(0.0),
                estimated_cost_usd: r.try_get("estimated_cost_usd").unwrap_or(None),
            }
        })
        .collect();

    let total_cost_usd: f64 = by_providers.iter()
        .filter_map(|b| b.estimated_cost_usd)
        .sum();

    Ok(Json(UsageBreakdownResponse { by_providers, by_key, by_model, total_cost_usd }))
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
}
