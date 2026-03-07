use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::analytics_repository::{
    AnalyticsSummary, FinishReasonStat, HourlyUsage, ModelStat, UsageAggregate, UsageJob,
};
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireSuper};

use super::error::AppError;
use super::state::AppState;

/// Percentage with one decimal place: `(numerator / denominator * 100)` rounded to 0.1.
fn pct(numerator: i64, denominator: i64) -> f64 {
    if denominator > 0 {
        (numerator as f64 / denominator as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    }
}

// ── Query parameters ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UsageQuery {
    #[serde(default = "default_hours")]
    pub hours: u32,
}

fn default_hours() -> u32 {
    24
}

// ── Helpers ────────────────────────────────────────────────────────

/// Verify the key belongs to the authenticated user.
async fn verify_key_ownership(
    state: &AppState,
    claims: &Claims,
    key_id: &uuid::Uuid,
) -> Result<(), AppError> {
    let tenant_id = super::key_handlers::resolve_tenant_id(state, claims).await?;
    let key = state
        .api_key_repo
        .get_by_id(key_id)
        .await?
        .ok_or_else(|| AppError::NotFound("key not found".into()))?;
    if key.tenant_id != tenant_id {
        return Err(AppError::Forbidden("access denied".into()));
    }
    Ok(())
}

/// Validate hours parameter to prevent SQL INTERVAL injection.
fn validate_hours(hours: u32) -> Result<(), AppError> {
    if hours > 8760 {
        return Err(AppError::BadRequest("hours must be <= 8760".into()));
    }
    Ok(())
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/usage — Aggregate usage across all keys (super admin only).
/// ClickHouse primary, PostgreSQL fallback.
pub async fn aggregate_usage(
    RequireSuper(_): RequireSuper,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageAggregate>, AppError> {
    validate_hours(params.hours)?;
    if let Some(repo) = state.analytics_repo.as_ref() {
        if let Ok(result) = repo.aggregate_usage(params.hours).await {
            if result.request_count > 0 {
                return Ok(Json(result));
            }
        }
    }
    Ok(Json(pg_aggregate_usage(&state.pg_pool, params.hours).await?))
}

/// GET /v1/usage/{key_id} — Per-key hourly breakdown.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn key_usage(
    Extension(claims): Extension<Claims>,
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<HourlyUsage>>, AppError> {
    validate_hours(params.hours)?;
    let uuid = super::handlers::parse_uuid(&key_id)?;
    verify_key_ownership(&state, &claims, &uuid).await?;
    if let Some(repo) = state.analytics_repo.as_ref() {
        if let Ok(rows) = repo.key_usage_hourly(&uuid, params.hours).await {
            if !rows.is_empty() {
                return Ok(Json(rows));
            }
        }
    }
    Ok(Json(pg_key_usage_hourly(&state.pg_pool, &uuid, params.hours).await?))
}

/// GET /v1/dashboard/analytics — Model distribution, finish reasons, TPS and avg tokens (super admin only).
/// ClickHouse primary, PostgreSQL fallback.
pub async fn get_analytics(
    RequireSuper(_): RequireSuper,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<AnalyticsSummary>, AppError> {
    validate_hours(params.hours)?;
    if let Some(repo) = state.analytics_repo.as_ref() {
        if let Ok(summary) = repo.analytics_summary(params.hours).await {
            if !summary.models.is_empty() {
                return Ok(Json(summary));
            }
        }
    }
    Ok(Json(pg_analytics_summary(&state.pg_pool, params.hours).await?))
}

/// GET /v1/usage/{key_id}/jobs — Individual request list for a key.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn key_usage_jobs(
    Extension(claims): Extension<Claims>,
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<UsageJob>>, AppError> {
    validate_hours(params.hours)?;
    let uuid = super::handlers::parse_uuid(&key_id)?;
    verify_key_ownership(&state, &claims, &uuid).await?;
    if let Some(repo) = state.analytics_repo.as_ref() {
        if let Ok(jobs) = repo.key_usage_jobs(&uuid, params.hours).await {
            if !jobs.is_empty() {
                return Ok(Json(jobs));
            }
        }
    }
    Ok(Json(pg_key_usage_jobs(&state.pg_pool, &uuid, params.hours).await?))
}

/// GET /v1/usage/{key_id}/models — Per-key model breakdown from PostgreSQL.
/// Returns which models the key has used, with request counts and token stats.
pub async fn key_model_breakdown(
    Extension(claims): Extension<Claims>,
    Path(key_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<ModelBreakdown>>, AppError> {
    use sqlx::Row;

    validate_hours(params.hours)?;
    let uuid = super::handlers::parse_uuid(&key_id)?;
    verify_key_ownership(&state, &claims, &uuid).await?;
    let pool = &state.pg_pool;

    let total_row = sqlx::query(
        "SELECT COUNT(*) AS total FROM inference_jobs
         WHERE api_key_id = $1
           AND created_at > NOW() - make_interval(hours => $2)"
    )
    .bind(uuid)
    .bind(params.hours as f64)
    .fetch_one(pool)
    .await?;
    let total: i64 = total_row.try_get("total").unwrap_or(1).max(1);

    let rows = sqlx::query(&format!(
        "SELECT
            j.model_name,
            j.provider_type,
            COUNT(*)                                                                           AS request_count,
            COUNT(*) FILTER (WHERE j.status = 'completed')                                    AS success_count,
            COALESCE(SUM(j.prompt_tokens), 0)                                                  AS prompt_tokens,
            COALESCE(SUM(j.completion_tokens), 0)                                              AS completion_tokens,
            COALESCE(AVG(j.latency_ms) FILTER (WHERE j.status = 'completed' AND j.latency_ms > 0), 0)::float8 AS avg_latency_ms,
            CASE
                WHEN j.provider_type = 'ollama' THEN 0.0
                WHEN pricing.input_per_1m IS NOT NULL THEN
                    (COALESCE(SUM(j.prompt_tokens), 0)::float8     / 1000000.0 * pricing.input_per_1m) +
                    (COALESCE(SUM(j.completion_tokens), 0)::float8 / 1000000.0 * pricing.output_per_1m)
                ELSE NULL
            END AS estimated_cost_usd
         FROM inference_jobs j
         {PRICING_LATERAL}
         WHERE j.api_key_id = $1
           AND j.created_at > NOW() - make_interval(hours => $2)
         GROUP BY j.model_name, j.provider_type, pricing.input_per_1m, pricing.output_per_1m
         ORDER BY request_count DESC
         LIMIT 50"
    ))
    .bind(uuid)
    .bind(params.hours as f64)
    .fetch_all(pool)
    .await?;

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

/// LATERAL JOIN for per-model pricing lookup. Used by key, model, and per-key breakdowns.
const PRICING_LATERAL: &str = "\
LEFT JOIN LATERAL (
    SELECT input_per_1m, output_per_1m FROM model_pricing
    WHERE provider = j.provider_type
      AND (model_name = j.model_name OR model_name = '*')
    ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
    LIMIT 1
) pricing ON true";

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

/// GET /v1/usage/breakdown — Provider, API key, and model breakdown from PostgreSQL (super admin only).
pub async fn usage_breakdown(
    RequireSuper(_): RequireSuper,
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageBreakdownResponse>, AppError> {
    use sqlx::Row;
    validate_hours(params.hours)?;
    let pool = &state.pg_pool;

    // ── By provider (with LATERAL pricing join) ────────────────────────
    let provider_rows = sqlx::query(
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
         WHERE j.created_at >= now() - make_interval(hours => $1)
         GROUP BY j.provider_type, j.provider_type, pricing.input_per_1m, pricing.output_per_1m
         ORDER BY request_count DESC",
    )
    .bind(params.hours as f64)
    .fetch_all(pool)
    .await?;

    let by_providers: Vec<ProviderBreakdown> = provider_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            let success_count: i64 = r.try_get("success_count").unwrap_or(0);
            ProviderBreakdown {
                provider_type: r.try_get("provider_type").unwrap_or_default(),
                request_count,
                success_count,
                error_count: r.try_get("error_count").unwrap_or(0),
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                success_rate: pct(success_count, request_count),
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
         {PRICING_LATERAL}
         WHERE j.created_at >= now() - make_interval(hours => $1)
         GROUP BY k.id, k.name, k.key_prefix
         ORDER BY request_count DESC",
    ))
    .bind(params.hours as f64)
    .fetch_all(pool)
    .await?;

    let by_key: Vec<KeyBreakdown> = key_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            let success_count: i64 = r.try_get("success_count").unwrap_or(0);
            KeyBreakdown {
                key_id: r.try_get("key_id").unwrap_or_default(),
                key_name: r.try_get("key_name").unwrap_or_default(),
                key_prefix: r.try_get("key_prefix").unwrap_or_default(),
                request_count,
                success_count,
                prompt_tokens: r.try_get("prompt_tokens").unwrap_or(0),
                completion_tokens: r.try_get("completion_tokens").unwrap_or(0),
                success_rate: pct(success_count, request_count),
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
            COALESCE(AVG(j.latency_ms) FILTER (WHERE j.latency_ms IS NOT NULL), 0)::float8 AS avg_latency_ms,
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
         {PRICING_LATERAL}
         WHERE j.created_at >= now() - make_interval(hours => $1)
         GROUP BY j.model_name, j.provider_type
         ORDER BY request_count DESC",
    ))
    .bind(params.hours as f64)
    .fetch_all(pool)
    .await?;

    let by_model: Vec<ModelBreakdown> = model_rows
        .iter()
        .map(|r| {
            let request_count: i64 = r.try_get("request_count").unwrap_or(0);
            ModelBreakdown {
                model_name: r.try_get("model_name").unwrap_or_default(),
                provider_type: r.try_get("provider_type").unwrap_or_default(),
                request_count,
                call_pct: pct(request_count, total_requests),
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

// ── PostgreSQL fallback queries ──────────────────────────────────────────────
//
// When the ClickHouse analytics pipeline is unavailable (analytics_repo = None
// or returns an error), these functions query the PostgreSQL `inference_jobs`
// table directly. The results match the same response types that ClickHouse
// returns, so the frontend sees identical JSON shapes.

async fn pg_aggregate_usage(pool: &sqlx::PgPool, hours: u32) -> Result<UsageAggregate, AppError> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT
            COUNT(*)                                           AS request_count,
            COUNT(*) FILTER (WHERE status = 'completed')      AS success_count,
            COUNT(*) FILTER (WHERE status = 'cancelled')      AS cancelled_count,
            COUNT(*) FILTER (WHERE status = 'failed')         AS error_count,
            COALESCE(SUM(prompt_tokens), 0)                   AS prompt_tokens,
            COALESCE(SUM(completion_tokens), 0)               AS completion_tokens,
            COALESCE(SUM(COALESCE(prompt_tokens,0) + COALESCE(completion_tokens,0)), 0) AS total_tokens
         FROM inference_jobs
         WHERE created_at >= NOW() - make_interval(hours => $1)"
    )
    .bind(hours as f64)
    .fetch_one(pool)
    .await?;

    Ok(UsageAggregate {
        request_count:    row.try_get::<i64, _>("request_count").unwrap_or(0) as u64,
        success_count:    row.try_get::<i64, _>("success_count").unwrap_or(0) as u64,
        cancelled_count:  row.try_get::<i64, _>("cancelled_count").unwrap_or(0) as u64,
        error_count:      row.try_get::<i64, _>("error_count").unwrap_or(0) as u64,
        prompt_tokens:    row.try_get::<i64, _>("prompt_tokens").unwrap_or(0) as u64,
        completion_tokens: row.try_get::<i64, _>("completion_tokens").unwrap_or(0) as u64,
        total_tokens:     row.try_get::<i64, _>("total_tokens").unwrap_or(0) as u64,
    })
}

async fn pg_key_usage_hourly(
    pool: &sqlx::PgPool,
    key_id: &uuid::Uuid,
    hours: u32,
) -> Result<Vec<HourlyUsage>, AppError> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT
            TO_CHAR(DATE_TRUNC('hour', created_at), 'YYYY-MM-DD\"T\"HH24:00:00\"Z\"') AS hour,
            COUNT(*)                                           AS request_count,
            COUNT(*) FILTER (WHERE status = 'completed')      AS success_count,
            COUNT(*) FILTER (WHERE status = 'cancelled')      AS cancelled_count,
            COUNT(*) FILTER (WHERE status = 'failed')         AS error_count,
            COALESCE(SUM(prompt_tokens), 0)                   AS prompt_tokens,
            COALESCE(SUM(completion_tokens), 0)               AS completion_tokens,
            COALESCE(SUM(COALESCE(prompt_tokens,0) + COALESCE(completion_tokens,0)), 0) AS total_tokens
         FROM inference_jobs
         WHERE api_key_id = $1
           AND created_at >= NOW() - make_interval(hours => $2)
         GROUP BY DATE_TRUNC('hour', created_at)
         ORDER BY hour"
    )
    .bind(key_id)
    .bind(hours as f64)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(|r| HourlyUsage {
        hour:              r.try_get("hour").unwrap_or_default(),
        request_count:     r.try_get::<i64, _>("request_count").unwrap_or(0) as u64,
        success_count:     r.try_get::<i64, _>("success_count").unwrap_or(0) as u64,
        cancelled_count:   r.try_get::<i64, _>("cancelled_count").unwrap_or(0) as u64,
        error_count:       r.try_get::<i64, _>("error_count").unwrap_or(0) as u64,
        prompt_tokens:     r.try_get::<i64, _>("prompt_tokens").unwrap_or(0) as u64,
        completion_tokens: r.try_get::<i64, _>("completion_tokens").unwrap_or(0) as u64,
        total_tokens:      r.try_get::<i64, _>("total_tokens").unwrap_or(0) as u64,
    }).collect())
}

async fn pg_analytics_summary(pool: &sqlx::PgPool, hours: u32) -> Result<AnalyticsSummary, AppError> {
    use sqlx::Row;
    // Aggregate stats
    let agg = sqlx::query(
        "SELECT
            COALESCE(AVG(
                CASE WHEN latency_ms > 0 AND completion_tokens > 0
                     THEN completion_tokens::float8 / (latency_ms::float8 / 1000.0)
                     ELSE NULL END
            ), 0) AS avg_tps,
            COALESCE(AVG(prompt_tokens)::float8, 0) AS avg_prompt_tokens,
            COALESCE(AVG(completion_tokens)::float8, 0) AS avg_completion_tokens
         FROM inference_jobs
         WHERE created_at >= NOW() - make_interval(hours => $1)
           AND status = 'completed'"
    )
    .bind(hours as f64)
    .fetch_one(pool)
    .await?;

    let avg_tps: f64 = agg.try_get("avg_tps").unwrap_or(0.0);
    let avg_prompt_tokens: f64 = agg.try_get("avg_prompt_tokens").unwrap_or(0.0);
    let avg_completion_tokens: f64 = agg.try_get("avg_completion_tokens").unwrap_or(0.0);

    // Per-model stats
    let model_rows = sqlx::query(
        "SELECT
            model_name,
            COUNT(*) AS request_count,
            COUNT(*) FILTER (WHERE status = 'completed') AS success_count,
            COALESCE(SUM(prompt_tokens), 0) AS total_prompt_tokens,
            COALESCE(SUM(completion_tokens), 0) AS total_completion_tokens,
            COALESCE(AVG(latency_ms) FILTER (WHERE status = 'completed' AND latency_ms > 0), 0)::float8 AS avg_latency_ms
         FROM inference_jobs
         WHERE created_at >= NOW() - make_interval(hours => $1)
         GROUP BY model_name
         ORDER BY request_count DESC
         LIMIT 50"
    )
    .bind(hours as f64)
    .fetch_all(pool)
    .await?;

    let models: Vec<ModelStat> = model_rows.iter().map(|r| {
        let req: i64 = r.try_get("request_count").unwrap_or(0);
        let suc: i64 = r.try_get("success_count").unwrap_or(0);
        ModelStat {
            model_name:             r.try_get("model_name").unwrap_or_default(),
            request_count:          req as u64,
            success_count:          suc as u64,
            success_rate:           pct(suc, req),
            total_prompt_tokens:    r.try_get::<i64, _>("total_prompt_tokens").unwrap_or(0) as u64,
            total_completion_tokens: r.try_get::<i64, _>("total_completion_tokens").unwrap_or(0) as u64,
            avg_latency_ms:         r.try_get("avg_latency_ms").unwrap_or(0.0),
        }
    }).collect();

    // Finish reasons (map job status to finish reasons)
    let reason_rows = sqlx::query(
        "SELECT status AS reason, COUNT(*) AS count
         FROM inference_jobs
         WHERE created_at >= NOW() - make_interval(hours => $1)
         GROUP BY status
         ORDER BY count DESC"
    )
    .bind(hours as f64)
    .fetch_all(pool)
    .await?;

    let finish_reasons: Vec<FinishReasonStat> = reason_rows.iter().map(|r| {
        let reason: String = r.try_get("reason").unwrap_or_default();
        FinishReasonStat {
            reason: match reason.as_str() {
                "completed" => "stop".to_string(),
                other => other.to_string(),
            },
            count: r.try_get::<i64, _>("count").unwrap_or(0) as u64,
        }
    }).collect();

    Ok(AnalyticsSummary {
        avg_tps,
        avg_prompt_tokens,
        avg_completion_tokens,
        models,
        finish_reasons,
    })
}

async fn pg_key_usage_jobs(
    pool: &sqlx::PgPool,
    key_id: &uuid::Uuid,
    hours: u32,
) -> Result<Vec<UsageJob>, AppError> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT
            created_at,
            id::TEXT AS request_id,
            model_name,
            COALESCE(prompt_tokens, 0) AS prompt_tokens,
            COALESCE(completion_tokens, 0) AS completion_tokens,
            COALESCE(latency_ms, 0) AS latency_ms,
            status
         FROM inference_jobs
         WHERE api_key_id = $1
           AND created_at >= NOW() - make_interval(hours => $2)
         ORDER BY created_at DESC
         LIMIT 500"
    )
    .bind(key_id)
    .bind(hours as f64)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(|r| {
        let status: String = r.try_get("status").unwrap_or_default();
        UsageJob {
            event_time:        r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_default(),
            request_id:        r.try_get("request_id").unwrap_or_default(),
            model_name:        r.try_get("model_name").unwrap_or_default(),
            prompt_tokens:     r.try_get::<i32, _>("prompt_tokens").unwrap_or(0) as u64,
            completion_tokens: r.try_get::<i32, _>("completion_tokens").unwrap_or(0) as u64,
            latency_ms:        r.try_get::<i32, _>("latency_ms").unwrap_or(0) as u64,
            finish_reason:     if status == "completed" { "stop".to_string() } else { status.clone() },
            status,
        }
    }).collect())
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
