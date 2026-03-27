//! Read-model queries for usage analytics.
//!
//! Each function takes a `&PgPool` and returns typed results.
//! SQL queries are copied exactly from the original handlers.

use serde::Serialize;

use crate::application::ports::outbound::analytics_repository::{
    AnalyticsSummary, FinishReasonStat, HourlyUsage, ModelStat, UsageAggregate, UsageJob,
};

use super::error::AppError;
use super::query_helpers::{pct, PRICING_LATERAL};

// ── Breakdown types ───────────────────────────────────────────────

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
    pub error_count: i64,
    pub cancelled_count: i64,
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

// ── Query functions ────────────────────────────────────────────────

pub(super) async fn pg_aggregate_usage(pool: &sqlx::PgPool, hours: u32) -> Result<UsageAggregate, AppError> {
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
    .bind(hours as i32)
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

pub(super) async fn pg_key_usage_hourly(
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
         ORDER BY hour
         LIMIT 8760"
    )
    .bind(key_id)
    .bind(hours as i32)
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

pub(super) async fn pg_analytics_summary(pool: &sqlx::PgPool, hours: u32) -> Result<AnalyticsSummary, AppError> {
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
    .bind(hours as i32)
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
    .bind(hours as i32)
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
         ORDER BY count DESC
         LIMIT 10"
    )
    .bind(hours as i32)
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

pub(super) async fn pg_key_usage_jobs(
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
    .bind(hours as i32)
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

/// Per-key model breakdown query.
pub(super) async fn pg_key_model_breakdown(
    pool: &sqlx::PgPool,
    key_id: &uuid::Uuid,
    hours: u32,
) -> Result<Vec<ModelBreakdown>, AppError> {
    use sqlx::Row;

    let total_row = sqlx::query(
        "SELECT COUNT(*) AS total FROM inference_jobs
         WHERE api_key_id = $1
           AND created_at > NOW() - make_interval(hours => $2)"
    )
    .bind(key_id)
    .bind(hours as i32)
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
    .bind(key_id)
    .bind(hours as i32)
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

    Ok(breakdown)
}

/// Full usage breakdown: by provider, by key, and by model.
pub(super) async fn pg_usage_breakdown(
    pool: &sqlx::PgPool,
    hours: u32,
) -> Result<UsageBreakdownResponse, AppError> {
    use sqlx::Row;

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
         GROUP BY j.provider_type, pricing.input_per_1m, pricing.output_per_1m
         ORDER BY request_count DESC
         LIMIT 50",
    )
    .bind(hours as i32)
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
            COUNT(j.id) FILTER (WHERE j.status = 'failed')        AS error_count,
            COUNT(j.id) FILTER (WHERE j.status = 'cancelled')     AS cancelled_count,
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
         ORDER BY request_count DESC
         LIMIT 500",
    ))
    .bind(hours as i32)
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
                error_count: r.try_get("error_count").unwrap_or(0),
                cancelled_count: r.try_get("cancelled_count").unwrap_or(0),
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
         ORDER BY request_count DESC
         LIMIT 200",
    ))
    .bind(hours as i32)
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

    Ok(UsageBreakdownResponse { by_providers, by_key, by_model, total_cost_usd })
}
