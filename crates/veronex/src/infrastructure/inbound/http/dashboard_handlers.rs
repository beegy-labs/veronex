use std::collections::HashMap;
use tracing::Instrument;
use std::convert::Infallible;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Deserializer, Serialize};

use crate::application::ports::outbound::analytics_repository::PerformanceMetrics;
use crate::domain::enums::AccountRole;
use crate::domain::value_objects::JobId;
use crate::infrastructure::outbound::valkey_keys as vk_keys;
use super::constants::{DASHBOARD_QUEUE_DEPTH_TIMEOUT, DASHBOARD_STATS_TIMEOUT};
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, RequireSettingsManage};
use crate::infrastructure::outbound::capacity::thermal::ThrottleLevel;
use crate::infrastructure::outbound::session_grouping::group_sessions_before;

use super::audit_helpers::emit_audit;
use super::constants::{PROVIDER_GEMINI, PROVIDER_OLLAMA};
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
                async { pool.llen::<i64, _>(vk_keys::queue_jobs_paid()).await.unwrap_or_else(|e| { tracing::warn!(error = %e, "queue depth: llen paid failed"); 0 }) },
                async { pool.llen::<i64, _>(vk_keys::queue_jobs()).await.unwrap_or_else(|e| { tracing::warn!(error = %e, "queue depth: llen api failed"); 0 }) },
                async { pool.llen::<i64, _>(vk_keys::queue_jobs_test()).await.unwrap_or_else(|e| { tracing::warn!(error = %e, "queue depth: llen test failed"); 0 }) },
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
        result.insert(PROVIDER_OLLAMA.to_string(), models);
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
            result.insert(PROVIDER_GEMINI.to_string(), gemini_models);
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

/// Deserializer for `Option<Option<T>>` that distinguishes absent from null:
/// - absent key  → `None`          (don't update this field)
/// - `null` value → `Some(None)`   (clear the field to NULL)
/// - value        → `Some(Some(v))` (set the field to v)
fn deserialize_nullable<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(serde::Deserialize)]
pub struct PatchLabSettingsBody {
    pub gemini_function_calling: Option<bool>,
    pub max_images_per_request: Option<i32>,
    pub max_image_b64_bytes: Option<i32>,
    pub context_compression_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_nullable")]
    pub compression_model: Option<Option<String>>,
    pub context_budget_ratio: Option<f32>,
    pub compression_trigger_turns: Option<i32>,
    pub recent_verbatim_window: Option<i32>,
    pub compression_timeout_secs: Option<i32>,
    pub multiturn_min_params: Option<i32>,
    pub multiturn_min_ctx: Option<i32>,
    pub multiturn_allowed_models: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable")]
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
    tokio::spawn(
        async move {
            let _permit = permit; // held until the task completes
            match group_sessions_before(&pg_pool, cutoff).await {
                Ok(n)  => tracing::info!(grouped = n, cutoff = ?cutoff, "manual session grouping complete"),
                Err(e) => tracing::warn!("manual session grouping failed: {e}"),
            }
        }
        .instrument(tracing::info_span!("veronex.dashboard_handlers.spawn")),
    );

    emit_audit(&state, &claims, "trigger", "session_grouping",
        "session_grouping", "session_grouping",
        &format!("Manual session grouping triggered (before: {:?})", cutoff)).await;

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "message": "session grouping triggered" })),
    )
        .into_response()
}

