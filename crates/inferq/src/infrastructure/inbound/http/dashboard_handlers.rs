use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::analytics_repository::PerformanceMetrics;
use crate::infrastructure::outbound::capacity::thermal::ThrottleLevel;
use crate::infrastructure::outbound::session_grouping::group_sessions_before;

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
    pub backend: String,
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

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/dashboard/stats — Overview statistics.
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<DashboardStats>, StatusCode> {
    let pool = &state.pg_pool;

    // Key counts (standard keys only — exclude test keys)
    let key_row = sqlx::query(
        "SELECT
            COUNT(*) FILTER (WHERE deleted_at IS NULL AND key_type != 'test') AS total_keys,
            COUNT(*) FILTER (WHERE is_active = true AND deleted_at IS NULL AND key_type = 'standard') AS active_keys
         FROM api_keys",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut jobs_by_status: HashMap<String, i64> = HashMap::new();
    for s in &["pending", "running", "completed", "failed", "cancelled"] {
        jobs_by_status.insert(s.to_string(), 0);
    }
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
    pub backend: String,
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

/// GET /v1/dashboard/jobs/{id} — Full job detail.
pub async fn get_job_detail(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Json<JobDetail>, StatusCode> {
    use sqlx::Row;
    let pool = &state.pg_pool;

    let row = sqlx::query(
        "SELECT j.id, j.model_name, j.backend, j.status, j.source,
                j.created_at, j.started_at, j.completed_at,
                j.latency_ms, j.ttft_ms, j.prompt_tokens, j.completion_tokens, j.cached_tokens,
                j.prompt, j.result_text, j.error, j.request_path,
                j.tool_calls_json,
                j.messages_json,
                COALESCE(jsonb_array_length(j.messages_json), 0) AS message_count,
                k.name AS api_key_name,
                a.name AS account_name,
                CASE
                    WHEN j.backend = 'ollama' THEN 0.0
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
             WHERE provider = j.backend
               AND (model_name = j.model_name OR model_name = '*')
             ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
             LIMIT 1
         ) pricing ON true
         WHERE j.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let id_val: uuid::Uuid = row.try_get("id").unwrap_or_default();
    let model_name: String = row.try_get("model_name").unwrap_or_default();
    let backend: String = row.try_get("backend").unwrap_or_default();
    let status: String = row.try_get("status").unwrap_or_default();
    let source: String = row.try_get("source").unwrap_or_else(|_| "api".to_string());
    let created_at: chrono::DateTime<chrono::Utc> = row.try_get("created_at").unwrap_or_default();
    let started_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("started_at").unwrap_or(None);
    let completed_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("completed_at").unwrap_or(None);
    let latency_ms: Option<i32> = row.try_get("latency_ms").unwrap_or(None);
    let ttft_ms: Option<i32> = row.try_get("ttft_ms").unwrap_or(None);
    let prompt_tokens: Option<i32> = row.try_get("prompt_tokens").unwrap_or(None);
    let completion_tokens: Option<i32> = row.try_get("completion_tokens").unwrap_or(None);
    let cached_tokens: Option<i32> = row.try_get("cached_tokens").unwrap_or(None);
    let api_key_name: Option<String> = row.try_get("api_key_name").unwrap_or(None);
    let account_name: Option<String> = row.try_get("account_name").unwrap_or(None);
    let prompt: String = row.try_get("prompt").unwrap_or_default();
    let result_text: Option<String> = row.try_get("result_text").unwrap_or(None);
    let error: Option<String> = row.try_get("error").unwrap_or(None);
    let request_path: Option<String> = row.try_get("request_path").unwrap_or(None);
    let tool_calls_json: Option<serde_json::Value> = row.try_get("tool_calls_json").unwrap_or(None);
    // messages_json: DB stores NULL for new jobs (S3 is authoritative).
    // Fall back to DB value for old jobs migrated before S3 was introduced.
    let db_messages: Option<serde_json::Value> = row.try_get("messages_json").unwrap_or(None);
    let message_count: Option<i32> = row.try_get("message_count").unwrap_or(None);
    let estimated_cost_usd: Option<f64> = row.try_get("estimated_cost_usd").unwrap_or(None);

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

    let tps = compute_tps(latency_ms, ttft_ms, completion_tokens);

    Ok(Json(JobDetail {
        id: id_val.to_string(),
        model_name,
        backend,
        status,
        source,
        created_at: created_at.to_rfc3339(),
        started_at: started_at.map(|dt| dt.to_rfc3339()),
        completed_at: completed_at.map(|dt| dt.to_rfc3339()),
        latency_ms: latency_ms.map(|v| v as i64),
        ttft_ms: ttft_ms.map(|v| v as i64),
        prompt_tokens: prompt_tokens.map(|v| v as i64),
        completion_tokens: completion_tokens.map(|v| v as i64),
        cached_tokens: cached_tokens.map(|v| v as i64),
        tps,
        api_key_name,
        account_name,
        prompt,
        result_text,
        error,
        request_path,
        tool_calls_json,
        messages_json,
        message_count: message_count.map(|v| v as i64),
        estimated_cost_usd,
    }))
}

/// GET /v1/dashboard/jobs — Paginated job list.
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(params): Query<JobsQuery>,
) -> Result<Json<JobsResponse>, StatusCode> {
    use sqlx::Row;
    let pool = &state.pg_pool;

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
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .try_get("cnt")
    .unwrap_or(0);

    let rows = sqlx::query(
        "SELECT j.id, j.model_name, j.backend, j.status, j.source,
                j.created_at, j.completed_at, j.latency_ms,
                j.ttft_ms, j.prompt_tokens, j.completion_tokens, j.cached_tokens,
                j.request_path,
                (j.tool_calls_json IS NOT NULL) AS has_tool_calls,
                k.name AS api_key_name,
                a.name AS account_name,
                CASE
                    WHEN j.backend = 'ollama' THEN 0.0
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
             WHERE provider = j.backend
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
    .bind(params.limit)
    .bind(params.offset)
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let jobs: Vec<JobSummary> = rows
        .iter()
        .map(|row| {
            let id: uuid::Uuid = row.try_get("id").unwrap_or_default();
            let model_name: String = row.try_get("model_name").unwrap_or_default();
            let backend: String = row.try_get("backend").unwrap_or_default();
            let status: String = row.try_get("status").unwrap_or_default();
            let source: String = row.try_get("source").unwrap_or_else(|_| "api".to_string());
            let created_at: chrono::DateTime<chrono::Utc> =
                row.try_get("created_at").unwrap_or_default();
            let completed_at: Option<chrono::DateTime<chrono::Utc>> =
                row.try_get("completed_at").unwrap_or(None);
            let latency_ms: Option<i32> = row.try_get("latency_ms").unwrap_or(None);
            let ttft_ms: Option<i32> = row.try_get("ttft_ms").unwrap_or(None);
            let prompt_tokens: Option<i32> = row.try_get("prompt_tokens").unwrap_or(None);
            let completion_tokens: Option<i32> = row.try_get("completion_tokens").unwrap_or(None);
            let cached_tokens: Option<i32> = row.try_get("cached_tokens").unwrap_or(None);
            let api_key_name: Option<String> = row.try_get("api_key_name").unwrap_or(None);
            let account_name: Option<String> = row.try_get("account_name").unwrap_or(None);
            let request_path: Option<String> = row.try_get("request_path").unwrap_or(None);
            let has_tool_calls: bool = row.try_get("has_tool_calls").unwrap_or(false);
            let estimated_cost_usd: Option<f64> = row.try_get("estimated_cost_usd").unwrap_or(None);
            let tps = compute_tps(latency_ms, ttft_ms, completion_tokens);

            JobSummary {
                id: id.to_string(),
                model_name,
                backend,
                status,
                source,
                created_at: created_at.to_rfc3339(),
                completed_at: completed_at.map(|dt| dt.to_rfc3339()),
                latency_ms: latency_ms.map(|v| v as i64),
                ttft_ms: ttft_ms.map(|v| v as i64),
                prompt_tokens: prompt_tokens.map(|v| v as i64),
                completion_tokens: completion_tokens.map(|v| v as i64),
                cached_tokens: cached_tokens.map(|v| v as i64),
                tps,
                api_key_name,
                account_name,
                request_path,
                has_tool_calls,
                estimated_cost_usd,
            }
        })
        .collect();

    Ok(Json(JobsResponse { jobs, total }))
}

/// DELETE /v1/dashboard/jobs/{id} — Admin cancel a job (JWT-protected).
pub async fn cancel_job(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, StatusCode> {
    use crate::domain::value_objects::JobId;
    let jid = JobId(id);
    state
        .use_case
        .cancel(&jid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

/// GET /v1/dashboard/performance — Latency percentiles + hourly throughput.
pub async fn get_performance(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<PerformanceMetrics>, StatusCode> {
    let repo = state
        .analytics_repo
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let metrics = repo
        .performance(params.hours)
        .await
        .map_err(|e| {
            tracing::warn!("performance query failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(metrics))
}

// ── Capacity API response types ─────────────────────────────────────

#[derive(Serialize)]
pub struct ModelCapacityInfo {
    pub model_name:           String,
    pub recommended_slots:    i16,
    pub active_slots:         u32,
    pub available_slots:      u32,
    pub vram_model_mb:        i32,
    pub vram_kv_per_slot_mb:  i32,
    pub avg_tokens_per_sec:   f64,
    pub avg_prefill_tps:      f64,
    pub p95_latency_ms:       f64,
    pub sample_count:         i32,
    pub llm_concern:          Option<String>,
    pub llm_reason:           Option<String>,
    pub updated_at:           String,
}

#[derive(Serialize)]
pub struct BackendCapacityInfo {
    pub backend_id:    String,
    pub backend_name:  String,
    pub thermal_state: String,
    pub temp_c:        Option<f32>,
    pub models:        Vec<ModelCapacityInfo>,
}

#[derive(Serialize)]
pub struct CapacityResponse {
    pub backends: Vec<BackendCapacityInfo>,
}

#[derive(Serialize)]
pub struct CapacitySettingsResponse {
    pub analyzer_model:      String,
    pub batch_enabled:       bool,
    pub batch_interval_secs: i32,
    pub last_run_at:         Option<String>,
    pub last_run_status:     Option<String>,
    pub available_models:    Vec<String>,
}

#[derive(Deserialize)]
pub struct PatchCapacitySettings {
    pub analyzer_model:      Option<String>,
    pub batch_enabled:       Option<bool>,
    pub batch_interval_secs: Option<i32>,
}

// ── GET /v1/dashboard/capacity ──────────────────────────────────────

pub async fn get_capacity(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let entries = match state.capacity_repo.list_all().await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("get_capacity: failed to list: {e}");
            return Json(CapacityResponse { backends: vec![] }).into_response();
        }
    };

    let backends_list = state.backend_registry.list_all().await.unwrap_or_default();
    let backend_name_map: HashMap<uuid::Uuid, String> = backends_list
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();

    // Group entries by backend
    let mut by_backend: HashMap<uuid::Uuid, Vec<_>> = HashMap::new();
    for entry in entries {
        by_backend.entry(entry.backend_id).or_default().push(entry);
    }

    let mut result: Vec<BackendCapacityInfo> = Vec::new();
    for (backend_id, models) in by_backend {
        let thermal_level = state.thermal.get(backend_id);
        let temp_c = state.thermal.temp_c(backend_id);
        let thermal_state = match thermal_level {
            ThrottleLevel::Normal => "normal",
            ThrottleLevel::Soft   => "soft",
            ThrottleLevel::Hard   => "hard",
        };

        let model_infos: Vec<ModelCapacityInfo> = models
            .into_iter()
            .map(|e| {
                let active    = state.slot_map.active_slots(backend_id, &e.model_name);
                let available = state.slot_map.available_slots(backend_id, &e.model_name);
                ModelCapacityInfo {
                    model_name:          e.model_name,
                    recommended_slots:   e.recommended_slots,
                    active_slots:        active,
                    available_slots:     available,
                    vram_model_mb:       e.vram_model_mb,
                    vram_kv_per_slot_mb: e.vram_kv_per_slot_mb,
                    avg_tokens_per_sec:  e.avg_tokens_per_sec,
                    avg_prefill_tps:     e.avg_prefill_tps,
                    p95_latency_ms:      e.p95_latency_ms,
                    sample_count:        e.sample_count,
                    llm_concern:         e.llm_concern,
                    llm_reason:          e.llm_reason,
                    updated_at:          e.updated_at.to_rfc3339(),
                }
            })
            .collect();

        result.push(BackendCapacityInfo {
            backend_id:   backend_id.to_string(),
            backend_name: backend_name_map
                .get(&backend_id)
                .cloned()
                .unwrap_or_else(|| backend_id.to_string()),
            thermal_state: thermal_state.to_string(),
            temp_c,
            models: model_infos,
        });
    }

    Json(CapacityResponse { backends: result }).into_response()
}

// ── GET /v1/dashboard/capacity/settings ────────────────────────────

pub async fn get_capacity_settings(
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    let settings = state.capacity_settings_repo.get().await.unwrap_or_default();

    // Fetch available models from Ollama /api/tags
    let available_models = fetch_ollama_tags(&state.analyzer_url).await;

    Json(CapacitySettingsResponse {
        analyzer_model:      settings.analyzer_model,
        batch_enabled:       settings.batch_enabled,
        batch_interval_secs: settings.batch_interval_secs,
        last_run_at:         settings.last_run_at.map(|t| t.to_rfc3339()),
        last_run_status:     settings.last_run_status,
        available_models,
    })
    .into_response()
}

// ── PATCH /v1/dashboard/capacity/settings ──────────────────────────

pub async fn patch_capacity_settings(
    State(state): State<AppState>,
    Json(body): Json<PatchCapacitySettings>,
) -> impl axum::response::IntoResponse {
    let updated = state
        .capacity_settings_repo
        .update_settings(
            body.analyzer_model.as_deref(),
            body.batch_enabled,
            body.batch_interval_secs,
        )
        .await;

    match updated {
        Ok(settings) => {
            let available_models = fetch_ollama_tags(&state.analyzer_url).await;
            Json(CapacitySettingsResponse {
                analyzer_model:      settings.analyzer_model,
                batch_enabled:       settings.batch_enabled,
                batch_interval_secs: settings.batch_interval_secs,
                last_run_at:         settings.last_run_at.map(|t| t.to_rfc3339()),
                last_run_status:     settings.last_run_status,
                available_models,
            })
            .into_response()
        }
        Err(e) => {
            tracing::warn!("patch_capacity_settings failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── POST /v1/dashboard/capacity/sync ───────────────────────────────

pub async fn trigger_capacity_sync(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    state.capacity_manual_trigger.notify_one();
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "message": "capacity analysis triggered" })),
    )
        .into_response()
}

// ── Helper: fetch Ollama model tags ────────────────────────────────

async fn fetch_ollama_tags(analyzer_url: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct TagsResponse { models: Vec<TagModel> }
    #[derive(serde::Deserialize)]
    struct TagModel { name: String }

    let client = reqwest::Client::new();
    let url = format!("{}/api/tags", analyzer_url.trim_end_matches('/'));
    match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
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
        async { pool.llen::<i64, _>("veronex:queue:jobs:paid").await.unwrap_or(0) },
        async { pool.llen::<i64, _>("veronex:queue:jobs").await.unwrap_or(0) },
        async { pool.llen::<i64, _>("veronex:queue:jobs:test").await.unwrap_or(0) },
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

type SseJobStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

pub async fn job_events_sse(State(state): State<AppState>) -> impl IntoResponse {
    let mut rx = state.job_event_tx.subscribe();

    let stream: SseJobStream = Box::pin(async_stream::stream! {
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

    (
        [("X-Accel-Buffering", "no")],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))),
    )
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
    State(state): State<AppState>,
    Json(body): Json<PatchLabSettingsBody>,
) -> impl axum::response::IntoResponse {
    match state.lab_settings_repo.update(body.gemini_function_calling).await {
        Ok(s) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "gemini_function_calling": s.gemini_function_calling,
                "updated_at": s.updated_at,
            })),
        )
            .into_response(),
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
    let pg_pool = state.pg_pool.clone();
    let cutoff  = body.before_date;
    tokio::spawn(async move {
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
