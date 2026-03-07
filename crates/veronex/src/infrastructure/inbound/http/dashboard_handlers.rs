use std::collections::HashMap;
use std::convert::Infallible;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::analytics_repository::{HourlyThroughput, PerformanceMetrics};
use crate::domain::enums::AccountRole;
use crate::infrastructure::outbound::valkey_keys::{QUEUE_JOBS_PAID as QUEUE_KEY_API_PAID, QUEUE_JOBS as QUEUE_KEY_API, QUEUE_JOBS_TEST as QUEUE_KEY_TEST};
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireSuper};
use crate::infrastructure::outbound::capacity::thermal::ThrottleLevel;
use crate::infrastructure::outbound::session_grouping::group_sessions_before;

use super::audit_helpers::emit_audit;
use super::constants::OLLAMA_HEALTH_CHECK_TIMEOUT;
use super::error::AppError;
use super::handlers::SseStream;
use super::state::AppState;
use super::usage_handlers::UsageQuery;

// ── Query parameters ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct JobsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub status: Option<String>,
    /// Full-text search on prompt (case-insensitive substring match).
    pub q: Option<String>,
    /// Filter by job source: "api" or "test". Omit for all sources.
    pub source: Option<String>,
}

fn default_limit() -> i64 {
    50
}

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
pub struct JobSummary {
    pub id: String,
    pub model_name: String,
    pub provider_type: String,
    pub status: String,
    pub source: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    /// Tokens per second (generation only, excluding TTFT).
    pub tps: Option<f64>,
    pub api_key_name: Option<String>,
    /// For test run jobs: the account that submitted the job.
    pub account_name: Option<String>,
    /// HTTP path of the inbound request, e.g. "/v1/chat/completions".
    pub request_path: Option<String>,
    /// True when the model responded with tool calls instead of (or in addition to) text.
    pub has_tool_calls: bool,
    /// Estimated API cost in USD. $0.00 for Ollama (self-hosted). None = no pricing data.
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Serialize)]
pub struct JobsResponse {
    pub jobs: Vec<JobSummary>,
    pub total: i64,
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Compute tokens-per-second for a job.
fn compute_tps(
    latency_ms: Option<i32>,
    ttft_ms: Option<i32>,
    completion_tokens: Option<i32>,
) -> Option<f64> {
    let tokens = completion_tokens? as f64;
    let lat = latency_ms? as f64;
    let gen_ms = lat - ttft_ms.unwrap_or(0) as f64;
    if gen_ms > 0.0 && tokens > 0.0 {
        Some((tokens * 1000.0 / gen_ms * 10.0).round() / 10.0)
    } else {
        None
    }
}

/// Common fields extracted from an `inference_jobs` row.
/// Both `JobSummary` (list) and `JobDetail` (single) share these columns.
struct JobRowCommon {
    id: uuid::Uuid,
    model_name: String,
    provider_type: String,
    status: String,
    source: String,
    created_at: chrono::DateTime<chrono::Utc>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    latency_ms: Option<i32>,
    ttft_ms: Option<i32>,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    cached_tokens: Option<i32>,
    api_key_name: Option<String>,
    account_name: Option<String>,
    request_path: Option<String>,
    estimated_cost_usd: Option<f64>,
}

impl JobRowCommon {
    // Note: unwrap_or_default used intentionally for dashboard resilience.
    // Individual row corruption should not break the dashboard list view.
    // Schema mismatches will surface as empty/default values in the UI.
    fn from_row(row: &sqlx::postgres::PgRow) -> Self {
        use sqlx::Row;
        Self {
            id:                row.try_get("id").unwrap_or_default(),
            model_name:        row.try_get("model_name").unwrap_or_default(),
            provider_type:     row.try_get("provider_type").unwrap_or_default(),
            status:            row.try_get("status").unwrap_or_default(),
            source:            row.try_get("source").unwrap_or_else(|_| "api".to_string()),
            created_at:        row.try_get("created_at").unwrap_or_default(),
            completed_at:      row.try_get("completed_at").unwrap_or(None),
            latency_ms:        row.try_get("latency_ms").unwrap_or(None),
            ttft_ms:           row.try_get("ttft_ms").unwrap_or(None),
            prompt_tokens:     row.try_get("prompt_tokens").unwrap_or(None),
            completion_tokens: row.try_get("completion_tokens").unwrap_or(None),
            cached_tokens:     row.try_get("cached_tokens").unwrap_or(None),
            api_key_name:      row.try_get("api_key_name").unwrap_or(None),
            account_name:      row.try_get("account_name").unwrap_or(None),
            request_path:      row.try_get("request_path").unwrap_or(None),
            estimated_cost_usd: row.try_get("estimated_cost_usd").unwrap_or(None),
        }
    }

    fn tps(&self) -> Option<f64> {
        compute_tps(self.latency_ms, self.ttft_ms, self.completion_tokens)
    }
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/dashboard/stats — Overview statistics.
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<DashboardStats>, AppError> {
    let pool = &state.pg_pool;

    // Key counts (standard keys only — exclude test keys)
    let key_row = sqlx::query(
        "SELECT
            COUNT(*) FILTER (WHERE deleted_at IS NULL AND key_type != 'test' AND tenant_id = 'default') AS total_keys,
            COUNT(*) FILTER (WHERE is_active = true AND deleted_at IS NULL AND key_type = 'standard' AND tenant_id = 'default') AS active_keys
         FROM api_keys",
    )
    .fetch_one(pool)
    .await?;

    use sqlx::Row;
    let total_keys: i64 = key_row.try_get("total_keys").unwrap_or(0);
    let active_keys: i64 = key_row.try_get("active_keys").unwrap_or(0);

    // Job counts (exclude test-source jobs from dashboard aggregates)
    let job_row = sqlx::query(
        "SELECT
            COUNT(*) AS total_jobs,
            COUNT(*) FILTER (WHERE created_at >= now() - interval '24 hours') AS jobs_last_24h
         FROM inference_jobs
         WHERE source != 'test'",
    )
    .fetch_one(pool)
    .await?;

    let total_jobs: i64 = job_row.try_get("total_jobs").unwrap_or(0);
    let jobs_last_24h: i64 = job_row.try_get("jobs_last_24h").unwrap_or(0);

    // Jobs by status (API jobs only)
    let status_rows = sqlx::query(
        "SELECT status, COUNT(*) AS cnt
         FROM inference_jobs
         WHERE source != 'test'
         GROUP BY status",
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

    Ok(Json(DashboardStats {
        total_keys,
        active_keys,
        total_jobs,
        jobs_last_24h,
        jobs_by_status,
    }))
}

// ── Job detail ─────────────────────────────────────────────────────

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
}

/// GET /v1/dashboard/jobs/{id} — Full job detail (tenant-scoped).
///
/// Super admins can view any job. Regular users can only view jobs
/// belonging to their own account (matched via `account_id` on the job).
pub async fn get_job_detail(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Json<JobDetail>, AppError> {
    use sqlx::Row;
    let pool = &state.pg_pool;

    let row = sqlx::query(
        "SELECT j.id, j.model_name, j.provider_type, j.status, j.source,
                j.created_at, j.started_at, j.completed_at,
                j.latency_ms, j.ttft_ms, j.prompt_tokens, j.completion_tokens, j.cached_tokens,
                j.prompt, j.result_text, j.error, j.request_path,
                j.tool_calls_json,
                j.messages_json,
                COALESCE(jsonb_array_length(j.messages_json), 0) AS message_count,
                k.name AS api_key_name,
                k.tenant_id AS key_tenant_id,
                a.name AS account_name,
                j.account_id,
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
    .await?
    .ok_or_else(|| AppError::NotFound(format!("job {id} not found")))?;

    // Tenant verification: non-super users can only view their own jobs.
    if claims.role != AccountRole::Super {
        let job_account_id: Option<uuid::Uuid> = row.try_get("account_id").unwrap_or(None);
        if job_account_id != Some(claims.sub) {
            return Err(AppError::Forbidden("access denied".into()));
        }
    }

    let c = JobRowCommon::from_row(&row);
    let tps = c.tps();
    let started_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("started_at").unwrap_or(None);
    let prompt: String = row.try_get("prompt").unwrap_or_default();
    let result_text: Option<String> = row.try_get("result_text").unwrap_or(None);
    let error: Option<String> = row.try_get("error").unwrap_or(None);
    let tool_calls_json: Option<serde_json::Value> = row.try_get("tool_calls_json").unwrap_or(None);
    // messages_json: DB stores NULL for new jobs (S3 is authoritative).
    // Fall back to DB value for old jobs migrated before S3 was introduced.
    let db_messages: Option<serde_json::Value> = row.try_get("messages_json").unwrap_or(None);
    let message_count: Option<i32> = row.try_get("message_count").unwrap_or(None);

    // Resolve messages: S3 first (authoritative for new jobs), DB fallback for old jobs
    let messages_json = if let Some(ref store) = state.message_store {
        match store.get(id).await {
            Ok(Some(v)) => Some(v),
            Ok(None) => db_messages, // not in S3 → use DB value (old job)
            Err(e) => {
                tracing::warn!(job_id = %id, "S3 message fetch failed (using DB fallback): {e}");
                db_messages
            }
        }
    } else {
        db_messages
    };

    Ok(Json(JobDetail {
        id: c.id.to_string(),
        model_name: c.model_name,
        provider_type: c.provider_type,
        status: c.status,
        source: c.source,
        created_at: c.created_at.to_rfc3339(),
        started_at: started_at.map(|dt| dt.to_rfc3339()),
        completed_at: c.completed_at.map(|dt| dt.to_rfc3339()),
        latency_ms: c.latency_ms.map(|v| v as i64),
        ttft_ms: c.ttft_ms.map(|v| v as i64),
        prompt_tokens: c.prompt_tokens.map(|v| v as i64),
        completion_tokens: c.completion_tokens.map(|v| v as i64),
        cached_tokens: c.cached_tokens.map(|v| v as i64),
        tps,
        api_key_name: c.api_key_name,
        account_name: c.account_name,
        prompt,
        result_text,
        error,
        request_path: c.request_path,
        tool_calls_json,
        messages_json,
        message_count: message_count.map(|v| v as i64),
        estimated_cost_usd: c.estimated_cost_usd,
    }))
}

/// GET /v1/dashboard/jobs — Paginated job list.
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(params): Query<JobsQuery>,
) -> Result<Json<JobsResponse>, AppError> {
    use sqlx::Row;
    let pool = &state.pg_pool;

    // Cap pagination to prevent abuse
    let limit = params.limit.clamp(1, 1000);
    let offset = params.offset.max(0);

    let status_filter = params.status.as_deref().filter(|s| !s.is_empty());
    let source_filter = params.source.as_deref().filter(|s| !s.is_empty());
    let search_like = params
        .q
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s));

    let total: i64 = sqlx::query(
        "SELECT COUNT(*) AS cnt
         FROM inference_jobs j
         LEFT JOIN api_keys k ON k.id = j.api_key_id
         WHERE ($1::TEXT IS NULL OR j.status = $1)
           AND ($2::TEXT IS NULL OR j.prompt ILIKE $2 OR k.name ILIKE $2)
           AND ($3::TEXT IS NULL OR j.source = $3)",
    )
    .bind(status_filter)
    .bind(search_like.as_deref())
    .bind(source_filter)
    .fetch_one(pool)
    .await?
    .try_get("cnt")
    .unwrap_or(0);

    let rows = sqlx::query(
        "SELECT j.id, j.model_name, j.provider_type, j.status, j.source,
                j.created_at, j.completed_at, j.latency_ms,
                j.ttft_ms, j.prompt_tokens, j.completion_tokens, j.cached_tokens,
                j.request_path,
                (j.tool_calls_json IS NOT NULL) AS has_tool_calls,
                k.name AS api_key_name,
                a.name AS account_name,
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
         LEFT JOIN LATERAL (
             SELECT input_per_1m, output_per_1m
             FROM model_pricing
             WHERE provider = j.provider_type
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
         WHERE ($1::TEXT IS NULL OR j.status = $1)
           AND ($2::TEXT IS NULL OR j.prompt ILIKE $2 OR k.name ILIKE $2)
           AND ($3::TEXT IS NULL OR j.source = $3)
         ORDER BY j.created_at DESC LIMIT $4 OFFSET $5",
    )
    .bind(status_filter)
    .bind(search_like.as_deref())
    .bind(source_filter)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let jobs: Vec<JobSummary> = rows
        .iter()
        .map(|row| {
            let c = JobRowCommon::from_row(row);
            let tps = c.tps();
            let has_tool_calls: bool = row.try_get("has_tool_calls").unwrap_or(false);

            JobSummary {
                id: c.id.to_string(),
                model_name: c.model_name,
                provider_type: c.provider_type,
                status: c.status,
                source: c.source,
                created_at: c.created_at.to_rfc3339(),
                completed_at: c.completed_at.map(|dt| dt.to_rfc3339()),
                latency_ms: c.latency_ms.map(|v| v as i64),
                ttft_ms: c.ttft_ms.map(|v| v as i64),
                prompt_tokens: c.prompt_tokens.map(|v| v as i64),
                completion_tokens: c.completion_tokens.map(|v| v as i64),
                cached_tokens: c.cached_tokens.map(|v| v as i64),
                tps,
                api_key_name: c.api_key_name,
                account_name: c.account_name,
                request_path: c.request_path,
                has_tool_calls,
                estimated_cost_usd: c.estimated_cost_usd,
            }
        })
        .collect();

    Ok(Json(JobsResponse { jobs, total }))
}

/// DELETE /v1/dashboard/jobs/{id} — Admin cancel a job (JWT-protected).
pub async fn cancel_job(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, AppError> {
    use crate::domain::value_objects::JobId;
    let jid = JobId(id);
    state
        .use_case
        .cancel(&jid)
        .await?;
    Ok(StatusCode::OK)
}

/// GET /v1/dashboard/performance — Latency percentiles + hourly throughput.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn get_performance(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<PerformanceMetrics>, AppError> {
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(metrics) = repo.performance(params.hours).await
            && metrics.total_requests > 0 {
                return Ok(Json(metrics));
            }
    Ok(Json(pg_performance(&state.pg_pool, params.hours).await?))
}

// ── PostgreSQL fallback for performance ─────────────────────────────

#[allow(clippy::unwrap_used)]
async fn pg_performance(pool: &sqlx::PgPool, hours: u32) -> Result<PerformanceMetrics, AppError> {
    use sqlx::Row;
    let hours_f64 = hours as f64;

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
    .bind(hours_f64)
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
         ORDER BY hour"
    )
    .bind(hours_f64)
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

// ── Capacity API response types ─────────────────────────────────────

#[derive(Serialize)]
pub struct LoadedModelInfo {
    pub model_name:        String,
    pub weight_mb:         i32,
    pub kv_per_request_mb: i32,
    pub active_requests:   u32,
    pub max_concurrent:    u32,
    pub llm_concern:       Option<String>,
    pub llm_reason:        Option<String>,
}

#[derive(Serialize)]
pub struct ProviderVramInfo {
    pub provider_id:     String,
    pub provider_name:   String,
    pub total_vram_mb:   u32,
    pub used_vram_mb:    u32,
    pub available_vram_mb: u32,
    pub thermal_state:   String,
    pub temp_c:          Option<f32>,
    pub loaded_models:   Vec<LoadedModelInfo>,
}

#[derive(Serialize)]
pub struct CapacityResponse {
    pub providers: Vec<ProviderVramInfo>,
}

#[derive(Serialize)]
pub struct SyncSettingsResponse {
    pub analyzer_model:     String,
    pub sync_enabled:       bool,
    pub sync_interval_secs: i32,
    pub probe_permits:      i32,
    pub probe_rate:         i32,
    pub last_run_at:        Option<String>,
    pub last_run_status:    Option<String>,
    pub available_models:   Vec<String>,
}

impl SyncSettingsResponse {
    fn from_settings(
        settings: crate::application::ports::outbound::capacity_settings_repository::CapacitySettings,
        available_models: Vec<String>,
    ) -> Self {
        Self {
            analyzer_model:     settings.analyzer_model,
            sync_enabled:       settings.sync_enabled,
            sync_interval_secs: settings.sync_interval_secs,
            probe_permits:      settings.probe_permits,
            probe_rate:         settings.probe_rate,
            last_run_at:        settings.last_run_at.map(|t| t.to_rfc3339()),
            last_run_status:    settings.last_run_status,
            available_models,
        }
    }
}

#[derive(Deserialize)]
pub struct PatchSyncSettings {
    pub analyzer_model:     Option<String>,
    pub sync_enabled:       Option<bool>,
    pub sync_interval_secs: Option<i32>,
    pub probe_permits:      Option<i32>,
    pub probe_rate:         Option<i32>,
}

// ── GET /v1/dashboard/capacity ──────────────────────────────────────

pub async fn get_capacity(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let entries = match state.capacity_repo.list_all().await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("get_capacity: failed to list: {e}");
            return Json(CapacityResponse { providers: vec![] }).into_response();
        }
    };

    let providers_list = state.provider_registry.list_all().await.unwrap_or_default();
    let provider_name_map: HashMap<uuid::Uuid, String> = providers_list
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();

    // Group entries by provider
    let mut by_provider: HashMap<uuid::Uuid, Vec<_>> = HashMap::new();
    for entry in entries {
        by_provider.entry(entry.provider_id).or_default().push(entry);
    }

    let mut result: Vec<ProviderVramInfo> = Vec::new();
    for (provider_id, models) in by_provider {
        let thermal_level = state.thermal.get(provider_id);
        let temp_c = state.thermal.temp_c(provider_id);
        let thermal_state = match thermal_level {
            ThrottleLevel::Normal => "normal",
            ThrottleLevel::Soft   => "soft",
            ThrottleLevel::Hard   => "hard",
        };

        let loaded_models: Vec<LoadedModelInfo> = models
            .into_iter()
            .map(|e| {
                let active = state.vram_pool.active_requests(provider_id, &e.model_name);
                let max_conc = state.vram_pool.max_concurrent(provider_id, &e.model_name);
                LoadedModelInfo {
                    model_name:        e.model_name,
                    weight_mb:         e.weight_mb,
                    kv_per_request_mb: e.kv_per_request_mb,
                    active_requests:   active,
                    max_concurrent:    max_conc,
                    llm_concern:       e.llm_concern,
                    llm_reason:        e.llm_reason,
                }
            })
            .collect();

        result.push(ProviderVramInfo {
            provider_id:     provider_id.to_string(),
            provider_name:   provider_name_map
                .get(&provider_id)
                .cloned()
                .unwrap_or_else(|| provider_id.to_string()),
            total_vram_mb:   state.vram_pool.total_vram_mb(provider_id),
            used_vram_mb:    state.vram_pool.used_vram_mb(provider_id),
            available_vram_mb: state.vram_pool.available_vram_mb(provider_id),
            thermal_state:   thermal_state.to_string(),
            temp_c,
            loaded_models,
        });
    }

    Json(CapacityResponse { providers: result }).into_response()
}

// ── GET /v1/dashboard/capacity/settings ────────────────────────────

pub async fn get_capacity_settings(
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    let settings = state.capacity_settings_repo.get().await.unwrap_or_default();

    // Fetch available models from Ollama /api/tags
    let available_models = fetch_ollama_tags(&state.http_client, &state.analyzer_url).await;

    Json(SyncSettingsResponse::from_settings(settings, available_models)).into_response()
}

// ── PATCH /v1/dashboard/capacity/settings ──────────────────────────

pub async fn patch_capacity_settings(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Json(body): Json<PatchSyncSettings>,
) -> impl axum::response::IntoResponse {
    let updated = state
        .capacity_settings_repo
        .update_settings(
            body.analyzer_model.as_deref(),
            body.sync_enabled,
            body.sync_interval_secs,
            body.probe_permits,
            body.probe_rate,
        )
        .await;

    match updated {
        Ok(settings) => {
            emit_audit(&state, &claims, "update", "capacity_settings", "capacity_settings", "capacity_settings",
                &format!("Sync settings updated: model={:?}, sync_enabled={:?}, sync_interval_secs={:?}",
                    body.analyzer_model, body.sync_enabled, body.sync_interval_secs)).await;
            let available_models = fetch_ollama_tags(&state.http_client, &state.analyzer_url).await;
            Json(SyncSettingsResponse::from_settings(settings, available_models)).into_response()
        }
        Err(e) => {
            tracing::warn!("patch_capacity_settings failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── POST /v1/dashboard/capacity/sync ───────────────────────────────

pub async fn trigger_capacity_sync(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    if state.sync_lock.available_permits() == 0 {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "message": "sync already in progress" })),
        )
            .into_response();
    }
    state.sync_trigger.notify_one();
    emit_audit(&state, &claims, "trigger", "capacity_settings", "capacity_settings", "provider_sync",
        "Manual provider sync triggered by admin").await;
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "message": "provider sync triggered" })),
    )
        .into_response()
}

// ── Helper: fetch Ollama model tags ────────────────────────────────

async fn fetch_ollama_tags(client: &reqwest::Client, analyzer_url: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct TagsResponse { models: Vec<TagModel> }
    #[derive(serde::Deserialize)]
    struct TagModel { name: String }

    let url = format!("{}/api/tags", analyzer_url.trim_end_matches('/'));
    match client
        .get(&url)
        .timeout(OLLAMA_HEALTH_CHECK_TIMEOUT)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<TagsResponse>()
            .await
            .map(|t| t.models.into_iter().map(|m| m.name).collect())
            .unwrap_or_default(),
        Err(_) => vec![],
    }
}

// ── GET /v1/dashboard/queue/depth — Valkey queue lengths ────────────

/// Returns the number of jobs currently waiting in each Valkey queue.
/// Polls `LLEN` on the three queue keys; returns zero counts when Valkey is unavailable.
#[derive(Serialize)]
pub struct QueueDepth {
    pub api_paid: i64,
    pub api: i64,
    pub test: i64,
    pub total: i64,
}

pub async fn get_queue_depth(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let Some(ref pool) = state.valkey_pool else {
        return Json(QueueDepth { api_paid: 0, api: 0, test: 0, total: 0 }).into_response();
    };

    use fred::prelude::*;

    let (paid, api, test): (i64, i64, i64) = tokio::join!(
        async { pool.llen::<i64, _>(QUEUE_KEY_API_PAID).await.unwrap_or(0) },
        async { pool.llen::<i64, _>(QUEUE_KEY_API).await.unwrap_or(0) },
        async { pool.llen::<i64, _>(QUEUE_KEY_TEST).await.unwrap_or(0) },
    );

    Json(QueueDepth {
        api_paid: paid,
        api,
        test,
        total: paid + api + test,
    })
    .into_response()
}

// ── GET /v1/dashboard/jobs/stream — Real-time job status SSE ───────
//
// Streams JobStatusEvent JSON objects as SSE data frames.
// The client receives one event per job state transition
// (pending → running → completed/failed/cancelled).
// JWT Bearer auth enforced by the dashboard router middleware.

pub async fn job_events_sse(State(state): State<AppState>) -> axum::response::Response {
    let mut rx = state.job_event_tx.subscribe();

    let stream: SseStream = Box::pin(async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok::<Event, Infallible>(Event::default().event("job_status").data(json));
                }
                // Lag-skip (RecvError::Lagged): continue receiving; channel closed = break
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    super::handlers::sse_response(stream)
}

// ── Lab feature settings ─────────────────────────────────────────────
//
// Experimental features are disabled by default.
// Enable them deliberately in Settings → Lab Features.

/// `GET /v1/dashboard/lab` — return current lab feature flags.
pub async fn get_lab_settings(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    match state.lab_settings_repo.get().await {
        Ok(s) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "gemini_function_calling": s.gemini_function_calling,
                "updated_at": s.updated_at,
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!("get_lab_settings: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct PatchLabSettingsBody {
    pub gemini_function_calling: Option<bool>,
}

/// `PATCH /v1/dashboard/lab` — update lab feature flags.
pub async fn patch_lab_settings(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Json(body): Json<PatchLabSettingsBody>,
) -> impl axum::response::IntoResponse {
    match state.lab_settings_repo.update(body.gemini_function_calling).await {
        Ok(s) => {
            emit_audit(&state, &claims, "update", "lab_settings", "lab_settings", "lab_settings",
                &format!("Lab feature flags updated: gemini_function_calling={:?}",
                    body.gemini_function_calling)).await;
            (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "gemini_function_calling": s.gemini_function_calling,
                    "updated_at": s.updated_at,
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("patch_lab_settings: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_query_defaults() {
        let json = serde_json::json!({});
        let q: JobsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(q.status.is_none());
        assert!(q.source.is_none());
    }

    #[test]
    fn jobs_query_with_status() {
        let json = serde_json::json!({ "status": "completed", "limit": 10, "offset": 20 });
        let q: JobsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.limit, 10);
        assert_eq!(q.offset, 20);
        assert_eq!(q.status.as_deref(), Some("completed"));
    }

    #[test]
    fn dashboard_stats_serialization() {
        let mut jobs_by_status = HashMap::new();
        jobs_by_status.insert("completed".to_string(), 100_i64);
        jobs_by_status.insert("failed".to_string(), 5_i64);

        let stats = DashboardStats {
            total_keys: 10,
            active_keys: 8,
            total_jobs: 105,
            jobs_last_24h: 20,
            jobs_by_status,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_keys"], 10);
        assert_eq!(json["active_keys"], 8);
    }
}

// ── POST /v1/dashboard/session-grouping/trigger ─────────────────────

/// Immediately runs the session grouping algorithm in a background task.
/// Optional `before_date` limits the cutoff to jobs created before that date (ISO 8601).
/// Defaults to today's midnight — never touches today's in-progress conversations.
#[derive(Deserialize)]
pub struct TriggerGroupingRequest {
    /// ISO 8601 date (e.g. "2026-03-01"). Jobs created before this date are grouped.
    /// Omit to use the default: today's midnight (all jobs before today).
    pub before_date: Option<NaiveDate>,
}

pub async fn trigger_session_grouping(
    State(state): State<AppState>,
    Json(body): Json<TriggerGroupingRequest>,
) -> impl IntoResponse {
    // Prevent concurrent runs — return 409 if already in progress.
    let permit = match state.session_grouping_lock.clone().try_acquire_owned() {
        Ok(p)  => p,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "message": "session grouping already in progress" })),
            )
                .into_response();
        }
    };

    let pg_pool = state.pg_pool.clone();
    let cutoff  = body.before_date;
    tokio::spawn(async move {
        let _permit = permit; // held until the task completes
        match group_sessions_before(&pg_pool, cutoff).await {
            Ok(n)  => tracing::info!(grouped = n, cutoff = ?cutoff, "manual session grouping complete"),
            Err(e) => tracing::warn!("manual session grouping failed: {e}"),
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "message": "session grouping triggered" })),
    )
        .into_response()
}
