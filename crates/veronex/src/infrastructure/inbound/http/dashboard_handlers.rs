use std::collections::HashMap;
use std::convert::Infallible;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::analytics_repository::PerformanceMetrics;
use crate::domain::enums::AccountRole;
use crate::domain::value_objects::JobId;
use crate::infrastructure::outbound::valkey_keys::{self as valkey_keys, QUEUE_JOBS_PAID as QUEUE_KEY_API_PAID, QUEUE_JOBS as QUEUE_KEY_API, QUEUE_JOBS_TEST as QUEUE_KEY_TEST};
use super::constants::{DASHBOARD_QUEUE_DEPTH_TIMEOUT, DASHBOARD_STATS_TIMEOUT};
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireSettingsManage, RequireDashboardView};
use crate::infrastructure::outbound::capacity::thermal::ThrottleLevel;
use crate::infrastructure::outbound::session_grouping::group_sessions_before;

use super::audit_helpers::emit_audit;
use super::dashboard_queries::{self, DashboardStats, JobDetail, JobsResponse};
use super::error::AppError;
use super::handlers::{SseStream, try_acquire_sse, ListPageParams};
use super::state::AppState;
use super::query_helpers::validate_hours;
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
    /// Filter by model name (exact match).
    pub model: Option<String>,
    /// Filter by provider name (exact match via JOIN).
    pub provider: Option<String>,
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
    Path(jid): Path<JobId>,
) -> Result<Json<JobDetail>, AppError> {
    let id = jid.0;
    let row = dashboard_queries::fetch_job_detail(&state.pg_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("job {id} not found")))?;

    // Tenant verification: non-super users can only view their own jobs.
    if claims.role != AccountRole::Super
        && row.account_id != Some(claims.sub)
    {
        return Err(AppError::Forbidden("access denied".into()));
    }

    // Fetch full conversation from S3 (prompt, messages, tool_calls, result)
    // S3 key uses conversation_id UUID if available, otherwise job_id
    let conversation = if let Some(ref store) = state.message_store {
        let owner_id = row.account_id.or(row.api_key_id).unwrap_or(id);
        let date = row.common.created_at.date_naive();
        let s3_key_id = row.conversation_id.unwrap_or(id);
        match store.get_conversation(owner_id, date, s3_key_id).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(job_id = %id, "S3 conversation fetch failed (non-fatal): {e}");
                None
            }
        }
    } else {
        None
    };

    // Resolve image_keys → URLs
    let image_urls = row.image_keys.as_ref().and_then(|keys| {
        state.image_store.as_ref().map(|store| {
            keys.iter().map(|k| store.url(k)).collect()
        })
    });

    Ok(Json(dashboard_queries::build_job_detail(row, conversation, image_urls)))
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
    let model_filter = params.model.as_deref().filter(|s| !s.is_empty());
    let provider_filter = params.provider.as_deref().filter(|s| !s.is_empty());
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
        model_filter,
        provider_filter,
    )
    .await?;

    Ok(Json(resp))
}

/// DELETE /v1/dashboard/jobs/{id} — Admin cancel a job (JWT-protected).
pub async fn cancel_job(
    RequireSettingsManage(claims): RequireSettingsManage,
    State(state): State<AppState>,
    Path(jid): Path<JobId>,
) -> Result<StatusCode, AppError> {
    state
        .use_case
        .cancel(&jid)
        .await?;
    emit_audit(&state, &claims, "cancel", "inference_job", &jid.to_string(), &jid.to_string(),
        &format!("job {} cancelled by admin", jid)).await;
    Ok(StatusCode::OK)
}

/// GET /v1/dashboard/performance — Latency percentiles + hourly throughput.
/// ClickHouse primary, PostgreSQL fallback.
pub async fn get_performance(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<PerformanceMetrics>, AppError> {
    let hours = params.effective_hours()?;
    validate_hours(hours)?;
    if let Some(repo) = state.analytics_repo.as_ref()
        && let Ok(metrics) = repo.performance(hours).await
            && metrics.total_requests > 0 {
                return Ok(Json(metrics));
            }
    Ok(Json(dashboard_queries::pg_performance(&state.pg_pool, hours).await?))
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
    pub total_vram_mb:   u64,
    pub used_vram_mb:    u64,
    pub available_vram_mb: u64,
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
    pub available_models:   HashMap<String, Vec<String>>,
}

impl SyncSettingsResponse {
    fn from_settings(
        settings: crate::application::ports::outbound::capacity_settings_repository::CapacitySettings,
        available_models: HashMap<String, Vec<String>>,
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
            ThrottleLevel::Normal   => "normal",
            ThrottleLevel::Soft     => "soft",
            ThrottleLevel::Hard     => "hard",
            ThrottleLevel::Cooldown => "cooldown",
            ThrottleLevel::RampUp   => "rampup",
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

    let (paid, api, test): (i64, i64, i64) = match tokio::time::timeout(
        DASHBOARD_QUEUE_DEPTH_TIMEOUT,
        async {
            tokio::join!(
                async { pool.llen::<i64, _>(QUEUE_KEY_API_PAID).await.unwrap_or_else(|e| { tracing::warn!(error = %e, "queue depth: llen paid failed"); 0 }) },
                async { pool.llen::<i64, _>(QUEUE_KEY_API).await.unwrap_or_else(|e| { tracing::warn!(error = %e, "queue depth: llen api failed"); 0 }) },
                async { pool.llen::<i64, _>(QUEUE_KEY_TEST).await.unwrap_or_else(|e| { tracing::warn!(error = %e, "queue depth: llen test failed"); 0 }) },
            )
        },
    )
    .await {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!("queue depth: Valkey timeout after 3s");
            (0, 0, 0)
        }
    };

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
    pub max_images_per_request: i32,
    pub max_image_b64_bytes: i32,
    pub context_compression_enabled: bool,
    pub compression_model: Option<String>,
    pub context_budget_ratio: f32,
    pub compression_trigger_turns: i32,
    pub recent_verbatim_window: i32,
    pub compression_timeout_secs: i32,
    pub multiturn_min_params: i32,
    pub multiturn_min_ctx: i32,
    pub multiturn_allowed_models: Vec<String>,
    pub vision_model: Option<String>,
    pub handoff_enabled: bool,
    pub handoff_threshold: f32,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct DashboardOverview {
    pub stats: DashboardStats,
    pub performance: PerformanceMetrics,
    pub queue_depth: QueueDepth,
    pub lab: LabSettingsResponse,
}

/// `GET /v1/dashboard/overview` — single aggregated snapshot of the entire dashboard.
///
/// Runs stats, performance, queue depth, and lab settings queries in parallel.
/// Capacity data is served by the dedicated `/capacity` endpoint (paginated).
pub async fn get_dashboard_overview(
    State(state): State<AppState>,
) -> Result<Json<DashboardOverview>, AppError> {
    let default_hours: u32 = 24;

    let result = tokio::time::timeout(
        DASHBOARD_STATS_TIMEOUT,
        async {
            tokio::join!(
                dashboard_queries::fetch_stats(&state.pg_pool),
                async {
                    if let Some(repo) = state.analytics_repo.as_ref()
                        && let Ok(metrics) = repo.performance(default_hours).await
                        && metrics.total_requests > 0
                    {
                        return Ok(metrics);
                    }
                    dashboard_queries::pg_performance(&state.pg_pool, default_hours).await
                },
                fetch_queue_depth(&state),
                async { state.lab_settings_repo.get().await },
            )
        },
    )
    .await
    .map_err(|_| AppError::Internal(anyhow::anyhow!("overview timeout")))?;

    let (stats_res, perf_res, queue, lab_res) = result;
    let stats = stats_res?;
    let performance = perf_res?;
    let lab_settings = lab_res.unwrap_or_default();
    let lab = lab_settings_to_response(lab_settings);

    Ok(Json(DashboardOverview {
        stats,
        performance,
        queue_depth: queue,
        lab,
    }))
}

// ── GET /v1/dashboard/capacity ──────────────────────────────────────

pub async fn get_capacity(
    State(state): State<AppState>,
    Query(params): Query<ListPageParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let limit = params.limit.unwrap_or(20).clamp(1, 200);
    let offset = (page - 1) * limit;
    let search = params.search.as_deref().unwrap_or("").to_string();

    let (providers_page, total) = state
        .provider_registry
        .list_page(&search, None, limit, offset)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e.to_string())))?;

    let provider_ids: Vec<uuid::Uuid> = providers_page.iter().map(|p| p.id).collect();
    let all_entries = state
        .capacity_repo
        .list_by_providers(&provider_ids)
        .await
        .unwrap_or_default();

    let capacity = build_capacity(&state, all_entries, providers_page);
    Ok(Json(serde_json::json!({
        "providers": capacity.providers,
        "total": total,
        "page": page,
        "limit": limit,
    })))
}

// ── GET /v1/dashboard/capacity/cluster ─────────────────────────────

#[derive(Serialize)]
pub struct ClusterModelInfo {
    pub model_name:        String,
    pub weight_mb:         i32,
    pub kv_per_request_mb: i32,
    pub total_active:      u32,
    pub total_limit:       u32,
    pub provider_count:    u32,
}

/// `GET /v1/dashboard/capacity/cluster`
///
/// Aggregates all loaded models across all providers from the in-memory VramPool.
/// Returns one row per unique model name with summed active/limit counts.
///
/// Reads entirely from the in-memory VramPool (no DB scan) — safe at 10K providers.
pub async fn get_capacity_cluster(
    State(state): State<AppState>,
) -> Result<Json<Vec<ClusterModelInfo>>, AppError> {
    let mut result: Vec<ClusterModelInfo> = state
        .vram_pool
        .cluster_snapshot()
        .into_iter()
        .map(|(model_name, weight_mb, kv_per_request_mb, total_active, total_limit, provider_count)| {
            ClusterModelInfo {
                model_name,
                weight_mb:         weight_mb.min(i32::MAX as u64) as i32,
                kv_per_request_mb: kv_per_request_mb.min(i32::MAX as u64) as i32,
                total_active,
                total_limit,
                provider_count,
            }
        })
        .collect();

    result.sort_by(|a, b| a.model_name.cmp(&b.model_name));
    Ok(Json(result))
}

// ── GET /v1/dashboard/capacity/settings ────────────────────────────

pub async fn get_capacity_settings(
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    let settings = state.capacity_settings_repo.get().await.unwrap_or_default();
    let available_models = fetch_all_provider_models(&state).await;
    Json(SyncSettingsResponse::from_settings(settings, available_models)).into_response()
}

// ── PATCH /v1/dashboard/capacity/settings ──────────────────────────

pub async fn patch_capacity_settings(
    RequireSettingsManage(claims): RequireSettingsManage,
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
            let available_models = fetch_all_provider_models(&state).await;
            Json(SyncSettingsResponse::from_settings(settings, available_models)).into_response()
        }
        Err(e) => {
            tracing::warn!("patch_capacity_settings failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── Helper: fetch models from all registered providers ────────────

async fn fetch_all_provider_models(state: &AppState) -> HashMap<String, Vec<String>> {
    let mut result: HashMap<String, Vec<String>> = HashMap::new();

    // ── Ollama: read from already-synced ollama_model_repo (no HTTP) ───
    if let Ok(models) = state.ollama_model_repo.list_all().await && !models.is_empty() {
        result.insert("ollama".to_string(), models);
    }

    // ── Gemini: show models only when lab feature is enabled ──
    let lab = state.lab_settings_repo.get().await.unwrap_or_default();
    if lab.gemini_function_calling {
        // Try DB first (synced models)
        let mut gemini_models: Vec<String> = state.gemini_model_repo
            .list()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.model_name)
            .collect();

        // Fallback: fetch from Gemini API if DB is empty
        if gemini_models.is_empty()
            && let Ok(Some(api_key)) = state.gemini_sync_config_repo.get_api_key().await
            && let Ok(models) = super::gemini_helpers::fetch_gemini_models(&state.http_client, &api_key).await
        {
            gemini_models = models;
        }

        if !gemini_models.is_empty() {
            result.insert("gemini".to_string(), gemini_models);
        }
    }

    result
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
    // Enforce global SSE connection limit — prevents resource exhaustion.
    let _guard = match try_acquire_sse(&state.sse_connections) {
        Ok(g) => g,
        Err(r) => return r,
    };

    // Subscribe to both channels BEFORE reading the ring buffer to avoid missing
    // events that arrive between the snapshot read and live subscription.
    let mut job_rx   = state.job_event_tx.subscribe();
    let mut stats_rx = state.stats_tx.subscribe();

    // Snapshot the replay buffer (oldest → newest) — include server timestamp.
    let buffered: Vec<String> = {
        #[allow(clippy::expect_used)]  // RwLock poisoning = thread panic, propagate correctly
        let buf = state.event_ring_buffer.read().expect("ring buffer poisoned");
        buf.iter()
            .filter_map(|(e, ts)| {
                let mut v = serde_json::to_value(e).ok()?;
                v.as_object_mut()?.insert("ts".into(), (*ts).into());
                serde_json::to_string(&v).ok()
            })
            .collect()
    };

    let stream: SseStream = Box::pin(async_stream::stream! {
        let _ = &_guard; // hold connection counter guard alive for stream lifetime
        // 1. Replay buffered job events so the client sees a populated feed immediately.
        for json in buffered {
            yield Ok::<Event, Infallible>(Event::default().event("job_status").data(json));
        }

        // 2. Forward live job events AND stats ticks as they arrive.
        // tokio::select! without `biased;` uses pseudo-random branch selection —
        // ensures fair interleaving between high-frequency job events and 1Hz stats.
        loop {
            tokio::select! {
                result = job_rx.recv() => match result {
                    Ok(event) => {
                        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                        let json = match serde_json::to_value(&event) {
                            Ok(mut v) => {
                                v.as_object_mut().map(|o| o.insert("ts".into(), now_ms.into()));
                                serde_json::to_string(&v).unwrap_or_default()
                            }
                            Err(_) => continue,
                        };
                        yield Ok::<Event, Infallible>(Event::default().event("job_status").data(json));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                result = stats_rx.recv() => match result {
                    Ok(stats) => {
                        let Ok(json) = serde_json::to_string(&stats) else { continue };
                        yield Ok::<Event, Infallible>(Event::default().event("flow_stats").data(json));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
        }
    });

    super::handlers::sse_response(stream)
}

// ── Lab feature settings ─────────────────────────────────────────────

fn lab_settings_to_response(s: crate::application::ports::outbound::lab_settings_repository::LabSettings) -> LabSettingsResponse {
    LabSettingsResponse {
        gemini_function_calling: s.gemini_function_calling,
        max_images_per_request: s.max_images_per_request,
        max_image_b64_bytes: s.max_image_b64_bytes,
        context_compression_enabled: s.context_compression_enabled,
        compression_model: s.compression_model,
        context_budget_ratio: s.context_budget_ratio,
        compression_trigger_turns: s.compression_trigger_turns,
        recent_verbatim_window: s.recent_verbatim_window,
        compression_timeout_secs: s.compression_timeout_secs,
        multiturn_min_params: s.multiturn_min_params,
        multiturn_min_ctx: s.multiturn_min_ctx,
        multiturn_allowed_models: s.multiturn_allowed_models,
        vision_model: s.vision_model,
        handoff_enabled: s.handoff_enabled,
        handoff_threshold: s.handoff_threshold,
        updated_at: s.updated_at,
    }
}

/// `GET /v1/dashboard/lab` — return current lab feature flags.
pub async fn get_lab_settings(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    match state.lab_settings_repo.get().await {
        Ok(s) => Json(lab_settings_to_response(s)).into_response(),
        Err(e) => {
            tracing::warn!("get_lab_settings: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
             Json(serde_json::json!({"error": "internal error"}))).into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct PatchLabSettingsBody {
    pub gemini_function_calling: Option<bool>,
    pub max_images_per_request: Option<i32>,
    pub max_image_b64_bytes: Option<i32>,
    pub context_compression_enabled: Option<bool>,
    pub compression_model: Option<Option<String>>,
    pub context_budget_ratio: Option<f32>,
    pub compression_trigger_turns: Option<i32>,
    pub recent_verbatim_window: Option<i32>,
    pub compression_timeout_secs: Option<i32>,
    pub multiturn_min_params: Option<i32>,
    pub multiturn_min_ctx: Option<i32>,
    pub multiturn_allowed_models: Option<Vec<String>>,
    pub vision_model: Option<Option<String>>,
    pub handoff_enabled: Option<bool>,
    pub handoff_threshold: Option<f32>,
}

/// `PATCH /v1/dashboard/lab` — update lab feature flags.
pub async fn patch_lab_settings(
    RequireSettingsManage(claims): RequireSettingsManage,
    State(state): State<AppState>,
    Json(body): Json<PatchLabSettingsBody>,
) -> impl axum::response::IntoResponse {
    use crate::application::ports::outbound::lab_settings_repository::LabSettingsUpdate;
    let patch = LabSettingsUpdate {
        gemini_function_calling: body.gemini_function_calling,
        max_images_per_request: body.max_images_per_request,
        max_image_b64_bytes: body.max_image_b64_bytes,
        context_compression_enabled: body.context_compression_enabled,
        compression_model: body.compression_model,
        context_budget_ratio: body.context_budget_ratio,
        compression_trigger_turns: body.compression_trigger_turns,
        recent_verbatim_window: body.recent_verbatim_window,
        compression_timeout_secs: body.compression_timeout_secs,
        multiturn_min_params: body.multiturn_min_params,
        multiturn_min_ctx: body.multiturn_min_ctx,
        multiturn_allowed_models: body.multiturn_allowed_models,
        vision_model: body.vision_model,
        handoff_enabled: body.handoff_enabled,
        handoff_threshold: body.handoff_threshold,
    };
    match state.lab_settings_repo.update(patch).await {
        Ok(s) => {
            emit_audit(&state, &claims, "update", "lab_settings", "lab_settings", "lab_settings",
                &format!("Lab settings updated: gemini_function_calling={:?}, max_images={:?}",
                    body.gemini_function_calling, body.max_images_per_request)).await;
            Json(lab_settings_to_response(s)).into_response()
        }
        Err(e) => {
            tracing::warn!("patch_lab_settings: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

// ── MCP server call stats ────────────────────────────────────────────────────

/// GET /v1/mcp/stats?hours=N — Per-server MCP call statistics.
///
/// Queries ClickHouse via the analytics service and joins with Postgres
/// to attach server_id and server_name. Returns an empty list when
/// analytics is unavailable or no data exists.
pub async fn get_mcp_stats(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let hours = params.effective_hours()?;
    validate_hours(hours)?;

    let slug_stats = if let Some(repo) = state.analytics_repo.as_ref() {
        repo.mcp_server_stats(hours).await.unwrap_or_default()
    } else {
        return Ok(Json(vec![]));
    };

    if slug_stats.is_empty() {
        return Ok(Json(vec![]));
    }

    // Join with Postgres for server_id and server_name.
    #[derive(sqlx::FromRow)]
    struct McpRow { id: uuid::Uuid, name: String, slug: String }

    let pg_rows: Vec<McpRow> = sqlx::query_as("SELECT id, name, slug FROM mcp_servers ORDER BY name LIMIT 500")
        .fetch_all(&state.pg_pool)
        .await?;

    let pg_map: std::collections::HashMap<&str, &McpRow> =
        pg_rows.iter().map(|r| (r.slug.as_str(), r)).collect();

    let result = slug_stats.into_iter().map(|s| {
        let (server_id, server_name) = pg_map.get(s.server_slug.as_str())
            .map(|r| (r.id.to_string(), r.name.clone()))
            .unwrap_or_else(|| (String::new(), s.server_slug.clone()));

        let success_rate = if s.total_calls > 0 {
            s.success_count as f64 / s.total_calls as f64
        } else { 0.0 };

        serde_json::json!({
            "server_id": server_id,
            "server_name": server_name,
            "server_slug": s.server_slug,
            "total_calls": s.total_calls,
            "success_count": s.success_count,
            "error_count": s.error_count,
            "cache_hit_count": s.cache_hit_count,
            "timeout_count": s.timeout_count,
            "success_rate": success_rate,
            "avg_latency_ms": s.avg_latency_ms,
        })
    }).collect();

    Ok(Json(result))
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
    RequireSettingsManage(claims): RequireSettingsManage,
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

    emit_audit(&state, &claims, "trigger", "session_grouping",
        "session_grouping", "session_grouping",
        &format!("Manual session grouping triggered (before: {:?})", cutoff)).await;

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "message": "session grouping triggered" })),
    )
        .into_response()
}

// ── Service health (infrastructure + pods) ────────────────────────

#[derive(Serialize)]
pub struct ServiceHealthResponse {
    pub infrastructure: Vec<ServiceStatus>,
    pub api_pods: Vec<PodStatus>,
    pub agent_pods: Vec<PodStatus>,
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub name: String,
    /// "ok" | "degraded" | "unavailable"
    pub status: String,
    pub latency_ms: Option<u32>,
    pub checked_at: Option<i64>,
}

#[derive(Serialize)]
pub struct PodStatus {
    pub id: String,
    /// "online" | "offline"
    pub status: String,
    pub last_heartbeat_ms: Option<i64>,
}

/// Compact JSON stored by health_checker in per-instance HASH.
#[derive(serde::Deserialize)]
struct SvcProbeEntry {
    s: String,
    ms: u32,
    t: i64,
}

/// GET /v1/dashboard/services — Infrastructure services + HPA pod status.
pub async fn get_service_health(
    State(state): State<AppState>,
) -> Result<Json<ServiceHealthResponse>, AppError> {
    use fred::prelude::*;

    let pool = state.valkey_pool.as_ref()
        .ok_or_else(|| AppError::ServiceUnavailable("Valkey not configured".into()))?;

    // ── 1. Infrastructure: merge service probes from all pods ──────
    let instance_ids: Vec<String> = pool.smembers(valkey_keys::INSTANCES_SET).await
        .unwrap_or_default();

    // Pipeline HGETALL for each instance's service health HASH
    let mut all_probes: HashMap<String, Vec<SvcProbeEntry>> = HashMap::new();
    for iid in &instance_ids {
        let entries: HashMap<String, String> = pool
            .hgetall(valkey_keys::service_health(iid))
            .await
            .unwrap_or_default();
        for (svc_name, json) in entries {
            if let Ok(probe) = serde_json::from_str::<SvcProbeEntry>(&json) {
                all_probes.entry(svc_name).or_default().push(probe);
            }
        }
    }

    // Merge: any "ok" → ok, mixed → degraded, all error → unavailable
    let svc_names = ["postgresql", "valkey", "clickhouse", "s3", "vespa"];
    let infrastructure: Vec<ServiceStatus> = svc_names.iter().filter_map(|name| {
        let probes = all_probes.get(*name)?;
        let ok_count = probes.iter().filter(|p| p.s == "ok").count();
        let status = if ok_count == probes.len() {
            "ok"
        } else if ok_count > 0 {
            "degraded"
        } else {
            "unavailable"
        };
        // Use the most recent probe for latency/timestamp
        let latest = probes.iter().max_by_key(|p| p.t)?;
        Some(ServiceStatus {
            name: name.to_string(),
            status: status.to_string(),
            latency_ms: Some(latest.ms),
            checked_at: Some(latest.t),
        })
    }).collect();

    // ── 2. API pods: check heartbeat TTL ──────────────────────────
    // Self-healing: remove stale instances (no heartbeat) from the SET
    // so restarted pods don't leave ghost entries.
    let api_pods: Vec<PodStatus> = {
        let mut pods = Vec::with_capacity(instance_ids.len());
        let now_ms = chrono::Utc::now().timestamp_millis();
        for iid in &instance_ids {
            let ttl: i64 = pool.ttl(valkey_keys::heartbeat(iid)).await.unwrap_or(-2);
            if ttl <= 0 {
                // Heartbeat expired — stale instance, remove from SET
                let _: Result<(), _> = pool
                    .srem(valkey_keys::INSTANCES_SET, iid.as_str())
                    .await;
                continue;
            }
            // Estimate last heartbeat: heartbeat TTL is 30s, refreshed every 10s
            let elapsed_ms = (30 - ttl) * 1000;
            pods.push(PodStatus {
                id: iid.clone(),
                status: "online".into(),
                last_heartbeat_ms: Some(now_ms - elapsed_ms),
            });
        }
        pods
    };

    // ── 3. Agent pods: read from veronex:agent:instances SET ────────
    // Each agent pod registers itself via SADD + heartbeat key with TTL.
    // Stale entries (heartbeat expired) are auto-removed.
    let agent_ids: Vec<String> = pool
        .smembers(valkey_keys::AGENT_INSTANCES_SET)
        .await
        .unwrap_or_default();

    let agent_pods: Vec<PodStatus> = {
        let mut pods = Vec::with_capacity(agent_ids.len());
        let now_ms = chrono::Utc::now().timestamp_millis();
        for hostname in &agent_ids {
            let hb_key = valkey_keys::agent_heartbeat(hostname);
            let ttl: i64 = pool.ttl(&hb_key).await.unwrap_or(-2);
            if ttl <= 0 {
                // Heartbeat expired — stale agent, remove from SET
                let _: Result<(), _> = pool
                    .srem(valkey_keys::AGENT_INSTANCES_SET, hostname.as_str())
                    .await;
                continue;
            }
            let elapsed_ms = (180 - ttl) * 1000; // agent heartbeat TTL = 180s
            pods.push(PodStatus {
                id: hostname.clone(),
                status: "online".into(),
                last_heartbeat_ms: Some(now_ms - elapsed_ms),
            });
        }
        pods
    };

    Ok(Json(ServiceHealthResponse { infrastructure, api_pods, agent_pods }))
}

// ── ClickHouse HTTP query helper ─────────────────────────────────────────────

/// Send a query to ClickHouse via its HTTP GET interface.
/// Uses percent-encoding for query params to avoid `.query()` type issues.
async fn ch_get(
    client: &reqwest::Client,
    base_url: &str,
    user: &str,
    password: &str,
    query: &str,
) -> Option<reqwest::Response> {
    let url = format!(
        "{base_url}/?user={}&password={}&query={}",
        percent_encode(user),
        percent_encode(password),
        percent_encode(query),
    );
    client.get(&url).send().await.ok()
        .filter(|r| r.status().is_success())
}

fn percent_encode(s: &str) -> String {
    s.bytes().flat_map(|b| {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => vec![b as char],
            _ => format!("%{b:02X}").chars().collect(),
        }
    }).collect()
}

// ── Pipeline health (Kafka consumer lag + TPM) ────────────────────────────────
//
// GET /v1/dashboard/pipeline
//
// Data sources:
//   1. Redpanda Prometheus metrics (/metrics) → high_watermark per topic
//   2. ClickHouse system.kafka_consumers      → consumer_offset, last_poll, errors
//   3. ClickHouse otel_logs / otel_metrics    → row count in last 1 / 5 minutes (TPM)

#[derive(Serialize)]
pub struct TopicPipelineStats {
    pub topic: String,
    pub consumer_offset: i64,
    pub log_end_offset: i64,
    /// lag = log_end_offset − consumer_offset. Negative means offset data mismatch (treat as 0).
    pub lag: i64,
    /// Rows inserted into the destination table in the last 1 minute.
    pub tpm_1m: i64,
    /// Rows inserted in the last 5 minutes (÷5 = avg TPM).
    pub tpm_5m: i64,
    /// Seconds since ClickHouse last polled this topic (None if unknown).
    pub last_poll_secs: Option<i64>,
    /// Whether the consumer is currently active.
    pub is_active: bool,
    /// Latest exception text from ClickHouse consumer (truncated to 200 chars).
    pub last_error: Option<String>,
    /// Number of active consumer threads for this topic.
    pub consumer_count: u32,
}

#[derive(Serialize)]
pub struct PipelineHealthResponse {
    pub topics: Vec<TopicPipelineStats>,
    /// True when pipeline metrics are available (Redpanda + ClickHouse reachable).
    pub available: bool,
}

/// `GET /v1/dashboard/pipeline`
pub async fn get_pipeline_health(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<super::state::AppState>,
) -> impl IntoResponse {
    let Some(ref redpanda_admin_url) = state.kafka_broker_admin_url else {
        return Json(PipelineHealthResponse { topics: vec![], available: false }).into_response();
    };
    let Some(ref ch_url) = state.clickhouse_http_url else {
        return Json(PipelineHealthResponse { topics: vec![], available: false }).into_response();
    };

    let ch_user = state.clickhouse_user.as_deref().unwrap_or("default");
    let ch_pass = state.clickhouse_password.as_deref().unwrap_or("");
    let ch_db   = state.clickhouse_db.as_deref().unwrap_or("veronex");

    // ── 1. Redpanda Prometheus metrics → high-watermark per topic ──────────
    let metrics_url = format!("{redpanda_admin_url}/metrics");
    let metrics_text = match state.http_client.get(&metrics_url).send().await {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => String::new(),
    };

    // Parse `vectorized_cluster_partition_high_watermark{...,topic="otel-logs",...} 123`
    let mut high_watermarks: HashMap<String, i64> = HashMap::new();
    for line in metrics_text.lines() {
        if !line.starts_with("vectorized_cluster_partition_high_watermark{") { continue }
        // Extract topic="..." value
        let topic = line
            .split("topic=\"").nth(1)
            .and_then(|s| s.split('"').next())
            .map(|s| s.to_string());
        let value = line.rsplit(' ').next()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|f| f as i64);
        if let (Some(t), Some(v)) = (topic, value) && t.starts_with("otel-") {
            high_watermarks.insert(t, v);
        }
    }

    // ── 2. ClickHouse → consumer offsets + last_poll + errors ──────────────
    let consumer_query = format!(
        "SELECT \
            table, \
            arrayElement(assignments.topic, 1) AS topic, \
            arrayElement(assignments.current_offset, 1) AS consumer_offset, \
            last_poll_time, \
            if(length(exceptions.text) > 0, \
               substring(arrayElement(exceptions.text, length(exceptions.text)), 1, 200), \
               '') AS last_error \
         FROM system.kafka_consumers \
         WHERE database='{ch_db}' \
           AND table IN ('kafka_otel_logs', 'kafka_otel_metrics') \
         FORMAT JSONEachRow"
    );

    let ch_consumer_resp = ch_get(&state.http_client, ch_url, ch_user, ch_pass, &consumer_query).await;

    #[derive(serde::Deserialize)]
    struct ChConsumerRow {
        topic: String,
        consumer_offset: i64,
        last_poll_time: String,  // "2026-01-01 00:00:00"
        last_error: String,
    }

    let mut consumer_map: HashMap<String, ChConsumerRow> = HashMap::new();
    if let Some(resp) = ch_consumer_resp && let Ok(body) = resp.text().await {
        for line in body.lines() {
            if let Ok(row) = serde_json::from_str::<ChConsumerRow>(line) {
                consumer_map.insert(row.topic.clone(), row);
            }
        }
    }

    // ── 3. ClickHouse → consumer count per topic ──────────────────────────
    let consumer_count_query = format!(
        "SELECT table, count() AS cnt \
         FROM system.kafka_consumers \
         WHERE database='{ch_db}' \
           AND table IN ('kafka_otel_logs', 'kafka_otel_metrics') \
         GROUP BY table \
         FORMAT JSONEachRow"
    );

    let ch_count_resp = ch_get(&state.http_client, ch_url, ch_user, ch_pass, &consumer_count_query).await;

    #[derive(serde::Deserialize)]
    struct ChCountRow {
        table: String,
        cnt: u32,
    }

    // Map from ClickHouse table name → consumer count
    let table_to_topic = [
        ("kafka_otel_logs",    "otel-logs"),
        ("kafka_otel_metrics", "otel-metrics"),
    ];
    let mut consumer_count_map: HashMap<&str, u32> = HashMap::new();
    if let Some(resp) = ch_count_resp && let Ok(body) = resp.text().await {
        for line in body.lines() {
            if let Ok(row) = serde_json::from_str::<ChCountRow>(line)
                && let Some(&topic) = table_to_topic.iter().find(|(t, _)| *t == row.table).map(|(_, tp)| tp)
            {
                consumer_count_map.insert(topic, row.cnt);
            }
        }
    }

    // ── 4. ClickHouse → TPM (rows inserted in last 1/5 minutes) ────────────
    // Destination tables: kafka_otel_logs → otel_logs, kafka_otel_metrics → otel_metrics
    let tpm_query = format!(
        "SELECT 'otel-logs' AS topic, \
                countIf(timestamp >= now() - INTERVAL 1 MINUTE) AS t1m, \
                countIf(timestamp >= now() - INTERVAL 5 MINUTE) AS t5m \
         FROM {ch_db}.otel_logs \
         UNION ALL \
         SELECT 'otel-metrics', \
                countIf(timestamp >= now() - INTERVAL 1 MINUTE), \
                countIf(timestamp >= now() - INTERVAL 5 MINUTE) \
         FROM {ch_db}.otel_metrics \
         FORMAT JSONEachRow"
    );

    let ch_tpm_resp = ch_get(&state.http_client, ch_url, ch_user, ch_pass, &tpm_query).await;

    #[derive(serde::Deserialize)]
    struct ChTpmRow {
        topic: String,
        t1m: i64,
        t5m: i64,
    }

    let mut tpm_map: HashMap<String, (i64, i64)> = HashMap::new();
    if let Some(resp) = ch_tpm_resp && let Ok(body) = resp.text().await {
        for line in body.lines() {
            if let Ok(row) = serde_json::from_str::<ChTpmRow>(line) {
                tpm_map.insert(row.topic, (row.t1m, row.t5m));
            }
        }
    }

    // ── 5. Assemble response ───────────────────────────────────────────────
    let now = chrono::Utc::now();
    let topics_config = [
        ("otel-logs",    "kafka_otel_logs"),
        ("otel-metrics", "kafka_otel_metrics"),
    ];

    let topics: Vec<TopicPipelineStats> = topics_config.iter().map(|(topic, _table)| {
        let log_end_offset = high_watermarks.get(*topic).copied().unwrap_or(0);

        let (consumer_offset, last_poll_secs, is_active, last_error) = if let Some(row) = consumer_map.get(*topic) {
            let last_poll_secs = chrono::NaiveDateTime::parse_from_str(&row.last_poll_time, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| now.signed_duration_since(dt.and_utc()).num_seconds())
                .filter(|&s| s >= 0);
            let err = if row.last_error.is_empty() { None } else { Some(row.last_error.clone()) };
            let is_active = last_poll_secs.map(|s| s < 120).unwrap_or(false);
            (row.consumer_offset, last_poll_secs, is_active, err)
        } else {
            (0, None, false, None)
        };

        let lag = (log_end_offset - consumer_offset).max(0);
        let (tpm_1m, tpm_5m) = tpm_map.get(*topic).copied().unwrap_or((0, 0));

        let consumer_count = consumer_count_map.get(*topic).copied().unwrap_or(0);

        TopicPipelineStats {
            topic: topic.to_string(),
            consumer_offset,
            log_end_offset,
            lag,
            tpm_1m,
            tpm_5m,
            last_poll_secs,
            is_active,
            last_error,
            consumer_count,
        }
    }).collect();

    let available = !metrics_text.is_empty() || !consumer_map.is_empty();
    Json(PipelineHealthResponse { topics, available }).into_response()
}
