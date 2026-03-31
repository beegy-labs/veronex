//! Read-model queries for the dashboard.
//!
//! Each function takes a `&PgPool` (or `&AppState`) and returns typed results.
//! SQL queries are copied exactly from the original handlers — no changes.

use std::collections::HashMap;

use serde::Serialize;

use crate::application::ports::outbound::analytics_repository::{HourlyThroughput, PerformanceMetrics};
use crate::application::ports::outbound::message_store::ConversationRecord;

use super::error::AppError;
use super::query_helpers::{JobRowCommon, JobSummary, job_summary_from_common};

// ── Response types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DashboardStats {
    pub total_keys: i64,
    /// Active standard (non-test) keys.
    pub active_keys: i64,
    pub total_jobs: i64,
    pub jobs_last_24h: i64,
    pub jobs_by_status: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct JobsResponse {
    pub jobs: Vec<JobSummary>,
    pub total: i64,
}

#[derive(Serialize)]
pub struct JobDetail {
    pub id: String,
    pub model_name: String,
    pub provider_type: String,
    pub status: String,
    pub source: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub tps: Option<f64>,
    pub api_key_name: Option<String>,
    /// For test run jobs: the account that submitted the job.
    pub account_name: Option<String>,
    pub prompt: String,
    pub result_text: Option<String>,
    pub error: Option<String>,
    /// HTTP path of the inbound request, e.g. "/v1/chat/completions".
    pub request_path: Option<String>,
    /// Tool calls the model emitted (when it responded with function calls instead of text).
    pub tool_calls_json: Option<serde_json::Value>,
    /// Number of messages in the conversation context (messages_json array length).
    pub message_count: Option<i64>,
    /// Full conversation context sent to the model (messages_json JSONB array).
    pub messages_json: Option<serde_json::Value>,
    /// Estimated API cost in USD. $0.00 for Ollama (self-hosted). None = no pricing data.
    pub estimated_cost_usd: Option<f64>,
    /// Name of the Ollama server / provider that processed this job.
    pub provider_name: Option<String>,
    /// S3 keys for stored WebP images.
    pub image_keys: Option<Vec<String>>,
    /// Resolved URLs for image thumbnails/full images.
    pub image_urls: Option<Vec<String>>,
}

// ── Query functions ────────────────────────────────────────────────

/// NOTE(scale): Called per dashboard page load, not per-second. Acceptable at current scale.
/// At 10K+ providers / high request volume, pending/running counts should read from Valkey
/// atomic counters (JOBS_PENDING_COUNTER / JOBS_RUNNING_COUNTER) and jobs_by_status should
/// be cached or moved to a materialized view.
pub(super) async fn fetch_stats(pool: &sqlx::PgPool) -> Result<DashboardStats, AppError> {
    use sqlx::Row;

    // Key counts (standard keys only — exclude test keys)
    let key_row = sqlx::query(
        "SELECT
            COUNT(*) FILTER (WHERE deleted_at IS NULL AND key_type != 'test' AND tenant_id = 'default') AS total_keys,
            COUNT(*) FILTER (WHERE is_active = true AND deleted_at IS NULL AND key_type = 'standard' AND tenant_id = 'default') AS active_keys
         FROM api_keys",
    )
    .fetch_one(pool)
    .await?;

    let total_keys: i64 = key_row.try_get("total_keys").unwrap_or(0);
    let active_keys: i64 = key_row.try_get("active_keys").unwrap_or(0);

    // Job counts: use pg_class estimate for total_jobs (O(1) instead of full table scan).
    // Exact count on millions of rows is too expensive for a dashboard page load.
    let job_row = sqlx::query(
        "SELECT
            COALESCE((SELECT reltuples::bigint FROM pg_class WHERE relname = 'inference_jobs'), 0) AS total_jobs,
            COUNT(*) AS jobs_last_24h
         FROM inference_jobs
         WHERE source NOT IN ('test', 'analyzer')
           AND created_at >= now() - interval '24 hours'",
    )
    .fetch_one(pool)
    .await?;

    let total_jobs: i64 = job_row.try_get("total_jobs").unwrap_or(0);
    let jobs_last_24h: i64 = job_row.try_get("jobs_last_24h").unwrap_or(0);

    // Jobs by status: only count recent jobs (last 7 days) to avoid full table scan.
    // For exact pending/running counts, the stats ticker uses Valkey atomic counters.
    let status_rows = sqlx::query(
        "SELECT status, COUNT(*) AS cnt
         FROM inference_jobs
         WHERE source NOT IN ('test', 'analyzer')
           AND created_at >= now() - interval '7 days'
         GROUP BY status
         LIMIT 10",
    )
    .fetch_all(pool)
    .await?;

    let mut jobs_by_status: HashMap<String, i64> =
        ["pending", "running", "completed", "failed", "cancelled"]
            .into_iter()
            .map(|s| (s.to_owned(), 0i64))
            .collect();
    for row in status_rows {
        let status: String = row.try_get("status").unwrap_or_default();
        let cnt: i64 = row.try_get("cnt").unwrap_or(0);
        jobs_by_status.insert(status, cnt);
    }

    Ok(DashboardStats {
        total_keys,
        active_keys,
        total_jobs,
        jobs_last_24h,
        jobs_by_status,
    })
}

/// Raw row returned by the job detail query.
/// Large content columns (prompt, result_text, messages_json, tool_calls_json) were
/// removed from Postgres — they are read from S3 ConversationRecord in the handler.
pub(super) struct JobDetailRow {
    pub common: JobRowCommon,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub prompt_preview: Option<String>,
    pub error: Option<String>,
    pub account_id: Option<uuid::Uuid>,
    pub api_key_id: Option<uuid::Uuid>,
    pub provider_name: Option<String>,
    pub image_keys: Option<Vec<String>>,
    pub conversation_id: Option<uuid::Uuid>,
}

pub(super) async fn fetch_job_detail(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
) -> Result<Option<JobDetailRow>, AppError> {
    use sqlx::Row;

    let row = sqlx::query(
        "SELECT j.id, j.model_name, j.provider_type, j.status, j.source,
                j.created_at, j.started_at, j.completed_at,
                j.latency_ms, j.ttft_ms, j.prompt_tokens, j.completion_tokens, j.cached_tokens,
                j.prompt_preview, j.error, j.request_path,
                j.image_keys, j.api_key_id, j.conversation_id,
                k.name AS api_key_name,
                k.tenant_id AS key_tenant_id,
                a.name AS account_name,
                j.account_id,
                p.name AS provider_name,
                CASE
                    WHEN j.provider_type = 'ollama' THEN 0.0
                    WHEN pricing.input_per_1m IS NOT NULL
                         AND j.prompt_tokens IS NOT NULL
                         AND j.completion_tokens IS NOT NULL THEN
                        (j.prompt_tokens::float8 / 1000000.0 * pricing.input_per_1m) +
                        (j.completion_tokens::float8 / 1000000.0 * pricing.output_per_1m)
                    ELSE NULL
                END AS estimated_cost_usd
         FROM inference_jobs j
         LEFT JOIN api_keys k ON k.id = j.api_key_id
         LEFT JOIN accounts a ON a.id = j.account_id
         LEFT JOIN llm_providers p ON p.id = j.provider_id
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m
             FROM model_pricing
             WHERE provider = j.provider_type
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
         WHERE j.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let common = JobRowCommon::from_row(&row);
    Ok(Some(JobDetailRow {
        common,
        started_at: row.try_get("started_at").unwrap_or(None),
        prompt_preview: row.try_get("prompt_preview").unwrap_or(None),
        error: row.try_get("error").unwrap_or(None),
        account_id: row.try_get("account_id").unwrap_or(None),
        api_key_id: row.try_get("api_key_id").unwrap_or(None),
        provider_name: row.try_get("provider_name").unwrap_or(None),
        image_keys: row.try_get("image_keys").unwrap_or(None),
        conversation_id: row.try_get("conversation_id").unwrap_or(None),
    }))
}

/// Build the final `JobDetail` response from a `JobDetailRow` and S3 conversation.
///
/// `conversation` is `None` when S3 is unavailable or the object does not exist yet
/// (e.g. job still running). In that case, `prompt_preview` is used as the prompt fallback.
pub(super) fn build_job_detail(
    row: JobDetailRow,
    conversation: Option<ConversationRecord>,
    image_urls: Option<Vec<String>>,
) -> JobDetail {
    let c = row.common;
    let tps = c.tps();

    let message_count = conversation.as_ref()
        .map(|r| r.turns.len() as i64);

    JobDetail {
        id: c.id.to_string(),
        model_name: c.model_name,
        provider_type: c.provider_type,
        status: c.status,
        source: c.source,
        created_at: c.created_at.to_rfc3339(),
        started_at: row.started_at.map(|dt| dt.to_rfc3339()),
        completed_at: c.completed_at.map(|dt| dt.to_rfc3339()),
        latency_ms: c.latency_ms.map(|v| v as i64),
        ttft_ms: c.ttft_ms.map(|v| v as i64),
        prompt_tokens: c.prompt_tokens.map(|v| v as i64),
        completion_tokens: c.completion_tokens.map(|v| v as i64),
        cached_tokens: c.cached_tokens.map(|v| v as i64),
        tps,
        api_key_name: c.api_key_name,
        account_name: c.account_name,
        prompt: conversation.as_ref()
            .and_then(|r| r.turns.iter().find(|t| t.job_id == c.id).map(|t| t.prompt.clone()))
            .unwrap_or_else(|| row.prompt_preview.unwrap_or_default()),
        result_text: conversation.as_ref()
            .and_then(|r| r.turns.iter().find(|t| t.job_id == c.id).and_then(|t| t.result.clone())),
        error: row.error,
        request_path: c.request_path,
        tool_calls_json: conversation.as_ref()
            .and_then(|r| r.turns.iter().find(|t| t.job_id == c.id).and_then(|t| t.tool_calls.clone())),
        messages_json: conversation
            .and_then(|r| r.turns.into_iter().find(|t| t.job_id == c.id).and_then(|t| t.messages)),
        message_count,
        estimated_cost_usd: c.estimated_cost_usd,
        provider_name: row.provider_name,
        image_keys: row.image_keys,
        image_urls,
    }
}

/// Fetch paginated job list with optional status/source/search/model/provider filters.
/// NOTE(scale): Uses COUNT(*) for total + OFFSET pagination. At 10K+ scale, replace with
/// cursor-based pagination (WHERE created_at < $cursor ORDER BY created_at DESC LIMIT N)
/// and drop the total count query to avoid sequential scans.
pub(super) async fn fetch_jobs(
    pool: &sqlx::PgPool,
    limit: i64,
    offset: i64,
    status_filter: Option<&str>,
    source_filter: Option<&str>,
    search_like: Option<&str>,
    model_filter: Option<&str>,
    provider_filter: Option<&str>,
) -> Result<JobsResponse, AppError> {
    use sqlx::Row;

    let total: i64 = sqlx::query(
        "SELECT COUNT(*) AS cnt
         FROM inference_jobs j
         LEFT JOIN api_keys k ON k.id = j.api_key_id
         LEFT JOIN llm_providers p ON p.id = j.provider_id
         WHERE ($1::TEXT IS NULL OR j.status = ANY(string_to_array($1, ',')))
           AND ($2::TEXT IS NULL OR j.prompt_preview ILIKE $2 OR k.name ILIKE $2)
           AND ($3::TEXT IS NULL OR j.source = $3)
           AND ($6::TEXT IS NULL OR j.model_name = $6)
           AND ($7::TEXT IS NULL OR p.name = $7)",
    )
    .bind(status_filter)
    .bind(search_like)
    .bind(source_filter)
    .bind(limit)       // $4 not used in count but keeps bind positions aligned
    .bind(offset)      // $5 not used in count
    .bind(model_filter)
    .bind(provider_filter)
    .fetch_one(pool)
    .await?
    .try_get("cnt")
    .unwrap_or(0);

    let rows = sqlx::query(
        "SELECT j.id, j.model_name, j.provider_type, j.status, j.source,
                j.created_at, j.completed_at, j.latency_ms,
                j.ttft_ms, j.prompt_tokens, j.completion_tokens, j.cached_tokens,
                j.request_path,
                j.has_tool_calls,
                k.name AS api_key_name,
                a.name AS account_name,
                p.name AS provider_name,
                CASE
                    WHEN j.provider_type = 'ollama' THEN 0.0
                    WHEN pricing.input_per_1m IS NOT NULL
                         AND j.prompt_tokens IS NOT NULL
                         AND j.completion_tokens IS NOT NULL THEN
                        (j.prompt_tokens::float8 / 1000000.0 * pricing.input_per_1m) +
                        (j.completion_tokens::float8 / 1000000.0 * pricing.output_per_1m)
                    ELSE NULL
                END AS estimated_cost_usd
         FROM inference_jobs j
         LEFT JOIN api_keys k ON k.id = j.api_key_id
         LEFT JOIN accounts a ON a.id = j.account_id
         LEFT JOIN llm_providers p ON p.id = j.provider_id
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m
             FROM model_pricing
             WHERE provider = j.provider_type
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
         WHERE ($1::TEXT IS NULL OR j.status = ANY(string_to_array($1, ',')))
           AND ($2::TEXT IS NULL OR j.prompt_preview ILIKE $2 OR k.name ILIKE $2)
           AND ($3::TEXT IS NULL OR j.source = $3)
           AND ($6::TEXT IS NULL OR j.model_name = $6)
           AND ($7::TEXT IS NULL OR p.name = $7)
         ORDER BY j.created_at DESC LIMIT $4 OFFSET $5",
    )
    .bind(status_filter)
    .bind(search_like)
    .bind(source_filter)
    .bind(limit)
    .bind(offset)
    .bind(model_filter)
    .bind(provider_filter)
    .fetch_all(pool)
    .await?;

    let jobs: Vec<JobSummary> = rows
        .iter()
        .map(|row| {
            let c = JobRowCommon::from_row(row);
            let has_tool_calls: bool = row.try_get("has_tool_calls").unwrap_or(false);
            let provider_name: Option<String> = row.try_get("provider_name").unwrap_or(None);
            job_summary_from_common(c, has_tool_calls, provider_name)
        })
        .collect();

    Ok(JobsResponse { jobs, total })
}

// ── PostgreSQL fallback for performance ─────────────────────────────

/// NOTE(scale): PERCENTILE_CONT is O(N log N) — expensive on large tables.
/// This is the PostgreSQL fallback; ClickHouse is the primary path (checked first
/// in get_performance / get_dashboard_overview). At 10K+ scale, ensure ClickHouse
/// is always available so this fallback is never hit in production.
#[allow(clippy::unwrap_used)]
pub(super) async fn pg_performance(pool: &sqlx::PgPool, hours: u32) -> Result<PerformanceMetrics, AppError> {
    use sqlx::Row;
    let hours_i32 = hours as i32;

    // Percentiles + aggregates from completed jobs
    let agg = sqlx::query(
        "SELECT
            COALESCE(AVG(latency_ms)::float8, 0) AS avg_latency_ms,
            COALESCE(PERCENTILE_CONT(0.50) WITHIN GROUP (ORDER BY latency_ms)::float8, 0) AS p50,
            COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)::float8, 0) AS p95,
            COALESCE(PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency_ms)::float8, 0) AS p99,
            COUNT(*) AS total_requests,
            COUNT(*) FILTER (WHERE status = 'completed') AS success_count,
            COALESCE(SUM(COALESCE(prompt_tokens,0) + COALESCE(completion_tokens,0)), 0) AS total_tokens
         FROM inference_jobs
         WHERE created_at >= NOW() - make_interval(hours => $1)
           AND latency_ms IS NOT NULL AND latency_ms > 0"
    )
    .bind(hours_i32)
    .fetch_one(pool)
    .await?;

    let total_requests: i64 = agg.try_get("total_requests").unwrap_or(0);
    let success_count: i64 = agg.try_get("success_count").unwrap_or(0);
    let success_rate = if total_requests > 0 {
        (success_count as f64 / total_requests as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };

    // Hourly throughput
    let hourly_rows = sqlx::query(
        "SELECT
            TO_CHAR(DATE_TRUNC('hour', created_at), 'YYYY-MM-DD\"T\"HH24:00:00\"Z\"') AS hour,
            COUNT(*) AS request_count,
            COUNT(*) FILTER (WHERE status = 'completed') AS success_count,
            COALESCE(AVG(latency_ms) FILTER (WHERE latency_ms > 0), 0)::float8 AS avg_latency_ms,
            COALESCE(SUM(COALESCE(prompt_tokens,0) + COALESCE(completion_tokens,0)), 0) AS total_tokens
         FROM inference_jobs
         WHERE created_at >= NOW() - make_interval(hours => $1)
         GROUP BY DATE_TRUNC('hour', created_at)
         ORDER BY hour
         LIMIT 8760"
    )
    .bind(hours_i32)
    .fetch_all(pool)
    .await?;

    let hourly: Vec<HourlyThroughput> = hourly_rows.iter().map(|r| HourlyThroughput {
        hour:          r.try_get("hour").unwrap_or_default(),
        request_count: r.try_get::<i64, _>("request_count").unwrap_or(0) as u64,
        success_count: r.try_get::<i64, _>("success_count").unwrap_or(0) as u64,
        avg_latency_ms: r.try_get("avg_latency_ms").unwrap_or(0.0),
        total_tokens:  r.try_get::<i64, _>("total_tokens").unwrap_or(0) as u64,
    }).collect();

    Ok(PerformanceMetrics {
        avg_latency_ms: agg.try_get("avg_latency_ms").unwrap_or(0.0),
        p50_latency_ms: agg.try_get("p50").unwrap_or(0.0),
        p95_latency_ms: agg.try_get("p95").unwrap_or(0.0),
        p99_latency_ms: agg.try_get("p99").unwrap_or(0.0),
        total_requests: total_requests as u64,
        success_rate,
        total_tokens: agg.try_get::<i64, _>("total_tokens").unwrap_or(0) as u64,
        hourly,
    })
}
