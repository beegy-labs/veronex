use std::collections::HashMap;
use std::convert::Infallible;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::analytics_repository::PerformanceMetrics;
use crate::domain::enums::AccountRole;
use crate::infrastructure::outbound::valkey_keys::{QUEUE_JOBS_PAID as QUEUE_KEY_API_PAID, QUEUE_JOBS as QUEUE_KEY_API, QUEUE_JOBS_TEST as QUEUE_KEY_TEST};
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireSuper};
use crate::infrastructure::outbound::capacity::thermal::ThrottleLevel;
use crate::infrastructure::outbound::session_grouping::group_sessions_before;

use super::audit_helpers::emit_audit;
use super::constants::OLLAMA_HEALTH_CHECK_TIMEOUT;
use super::dashboard_queries::{self, DashboardStats, JobDetail, JobsResponse};
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

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/dashboard/stats — Overview statistics.
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<DashboardStats>, AppError> {
    Ok(Json(dashboard_queries::fetch_stats(&state.pg_pool).await?))
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
    let row = dashboard_queries::fetch_job_detail(&state.pg_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("job {id} not found")))?;

    // Tenant verification: non-super users can only view their own jobs.
    if claims.role != AccountRole::Super
        && row.account_id != Some(claims.sub)
    {
        return Err(AppError::Forbidden("access denied".into()));
    }

    // Resolve messages: S3 first (authoritative for new jobs), DB fallback for old jobs
    let db_messages = row.db_messages.clone();
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

    Ok(Json(dashboard_queries::build_job_detail(row, messages_json)))
}

/// GET /v1/dashboard/jobs — Paginated job list.
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(params): Query<JobsQuery>,
) -> Result<Json<JobsResponse>, AppError> {
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

    let resp = dashboard_queries::fetch_jobs(
        &state.pg_pool,
        limit,
        offset,
        status_filter,
        source_filter,
        search_like.as_deref(),
    )
    .await?;

    Ok(Json(resp))
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
    Ok(Json(dashboard_queries::pg_performance(&state.pg_pool, params.hours).await?))
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

// ── Capacity helper (returns typed result) ──────────────────────────

fn build_capacity(state: &AppState, entries: Vec<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>, providers_list: Vec<crate::domain::entities::LlmProvider>) -> CapacityResponse {
    let provider_name_map: HashMap<uuid::Uuid, String> = providers_list
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();

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

    CapacityResponse { providers: result }
}

/// Build queue depth from Valkey, returning typed struct (zero counts if Valkey unavailable).
async fn fetch_queue_depth(state: &AppState) -> QueueDepth {
    let Some(ref pool) = state.valkey_pool else {
        return QueueDepth { api_paid: 0, api: 0, test: 0, total: 0 };
    };

    use fred::prelude::*;

    let (paid, api, test): (i64, i64, i64) = tokio::join!(
        async { pool.llen::<i64, _>(QUEUE_KEY_API_PAID).await.unwrap_or(0) },
        async { pool.llen::<i64, _>(QUEUE_KEY_API).await.unwrap_or(0) },
        async { pool.llen::<i64, _>(QUEUE_KEY_TEST).await.unwrap_or(0) },
    );

    QueueDepth {
        api_paid: paid,
        api,
        test,
        total: paid + api + test,
    }
}

// ── GET /v1/dashboard/overview — Aggregated dashboard snapshot ──────

#[derive(Serialize)]
pub struct LabSettingsResponse {
    pub gemini_function_calling: bool,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct DashboardOverview {
    pub stats: DashboardStats,
    pub performance: PerformanceMetrics,
    pub capacity: CapacityResponse,
    pub queue_depth: QueueDepth,
    pub lab: LabSettingsResponse,
}

/// `GET /v1/dashboard/overview` — single aggregated snapshot of the entire dashboard.
///
/// Runs stats, performance, capacity, queue depth, and lab settings queries
/// in parallel and returns a combined JSON response.
pub async fn get_dashboard_overview(
    State(state): State<AppState>,
) -> Result<Json<DashboardOverview>, AppError> {
    let default_hours: u32 = 24;

    let (stats_res, perf_res, cap_entries, providers_list, queue, lab_res) = tokio::join!(
        dashboard_queries::fetch_stats(&state.pg_pool),
        async {
            // ClickHouse primary, PostgreSQL fallback (same logic as get_performance)
            if let Some(repo) = state.analytics_repo.as_ref()
                && let Ok(metrics) = repo.performance(default_hours).await
                && metrics.total_requests > 0
            {
                return Ok(metrics);
            }
            dashboard_queries::pg_performance(&state.pg_pool, default_hours).await
        },
        async { state.capacity_repo.list_all().await.unwrap_or_default() },
        async { state.provider_registry.list_all().await.unwrap_or_default() },
        fetch_queue_depth(&state),
        async { state.lab_settings_repo.get().await },
    );

    let stats = stats_res?;
    let performance = perf_res?;
    let capacity = build_capacity(&state, cap_entries, providers_list);

    let lab_settings = lab_res.unwrap_or_default();
    let lab = LabSettingsResponse {
        gemini_function_calling: lab_settings.gemini_function_calling,
        updated_at: lab_settings.updated_at,
    };

    Ok(Json(DashboardOverview {
        stats,
        performance,
        capacity,
        queue_depth: queue,
        lab,
    }))
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
    Json(build_capacity(&state, entries, providers_list)).into_response()
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
    Json(fetch_queue_depth(&state).await).into_response()
}

// ── GET /v1/dashboard/jobs/stream — Real-time job status SSE ───────

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
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
