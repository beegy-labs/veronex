use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::constants::OLLAMA_HEALTH_CHECK_TIMEOUT;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{LlmProviderStatus, ProviderType};
use crate::domain::value_objects::{GpuServerId, ProviderId};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::outbound::health_checker::check_provider;
use crate::infrastructure::outbound::valkey_keys;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::gemini_helpers;
use super::provider_validation::{parse_provider_type, validate_provider_url};
use super::state::AppState;

use super::constants::MODELS_CACHE_TTL;

// ── Model cache helpers ─────────────────────────────────────────────────────────

fn models_cache_key(id: Uuid) -> String {
    valkey_keys::provider_models(id)
}

/// Fetch the list of available models directly from the provider (bypasses cache).
///
/// * Ollama → `GET {url}/api/tags`
/// * Gemini → `GET https://generativelanguage.googleapis.com/v1beta/models?key={api_key}`
///   filtered to models that support `generateContent`.
async fn fetch_models_live(client: &reqwest::Client, provider: &LlmProvider) -> Result<Vec<String>> {
    match provider.provider_type {
        ProviderType::Ollama => {
            let url = format!("{}/api/tags", provider.url.trim_end_matches('/'));
            let json: serde_json::Value = client
                .get(&url)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("cannot reach ollama: {e}"))?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("ollama returned error: {e}"))?
                .json()
                .await
                .map_err(|e| anyhow::anyhow!("failed to parse ollama response: {e}"))?;

            let models = json["models"]
                .as_array()
                .map_or(&[] as &[_], |v| v)
                .iter()
                .filter_map(|m| m["name"].as_str().map(String::from))
                .collect();

            Ok(models)
        }

        ProviderType::Gemini => {
            let api_key = provider
                .api_key_encrypted
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("gemini provider has no api key stored"))?;

            gemini_helpers::fetch_gemini_models(client, api_key).await
        }
    }
}

/// Write models to the Valkey cache (fire-and-forget; errors are logged, not surfaced).
async fn store_models_cache(pool: &fred::clients::Pool, key: &str, models: &[String]) {
    use fred::prelude::*;

    let Ok(json_str) = serde_json::to_string(models) else {
        return;
    };
    if let Err(e) = pool
        .set::<String, _, _>(
            key,
            json_str,
            Some(Expiration::EX(MODELS_CACHE_TTL)),
            None,
            false,
        )
        .await
    {
        tracing::warn!(error = %e, "failed to cache model list");
    }
}

/// Read models from the Valkey cache. Returns `None` on miss or error.
async fn load_models_cache(pool: &fred::clients::Pool, key: &str) -> Option<Vec<String>> {
    use fred::prelude::*;

    let cached: Option<String> = match pool.get(key).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(key, error = %e, "models cache: Valkey get failed");
            None
        }
    };
    let json_str = cached?;
    serde_json::from_str::<Vec<String>>(&json_str).ok()
}

// ── Request / Response DTOs ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VerifyProviderRequest {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterProviderRequest {
    /// Human-readable label.
    pub name: String,
    /// `"ollama"` or `"gemini"`.
    pub provider_type: String,
    /// Required for Ollama. E.g. `"http://192.168.1.10:11434"`.
    pub url: Option<String>,
    /// Required for Gemini. Encrypted at rest via AES-256-GCM.
    pub api_key: Option<String>,
    /// GPU VRAM capacity in MiB (manual). 0 = unknown.
    pub total_vram_mb: Option<i64>,
    /// GPU index on the host (0-based). For metric correlation.
    pub gpu_index: Option<i16>,
    /// FK → gpu_servers. Optional; Gemini providers leave this null.
    pub server_id: Option<GpuServerId>,
    /// true = key is on a Google free-tier project.
    /// RPM/RPD limits are managed globally via `gemini_rate_limit_policies`.
    pub is_free_tier: Option<bool>,
    /// Ollama num_parallel setting. Default 4. Used as AIMD upper bound.
    pub num_parallel: Option<i16>,
}

/// Update request for `PATCH /v1/providers/{id}`.
///
/// The web UI pre-fills all current values before submission, so every field
/// is always present.  `gpu_index` / `server_id` = `null` explicitly clears them.
/// `api_key` = `null` or empty string keeps the existing stored key.
#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    pub name: String,
    /// Ollama URL. Leave empty for Gemini.
    pub url: Option<String>,
    /// Replace the stored key when non-empty; otherwise keep existing.
    pub api_key: Option<String>,
    pub total_vram_mb: Option<i64>,
    pub gpu_index: Option<i16>,
    pub server_id: Option<GpuServerId>,
    pub is_free_tier: Option<bool>,
    /// Ollama num_parallel setting.
    pub num_parallel: Option<i16>,
    /// Enable or disable the provider for routing.
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ProviderSummary {
    pub id: ProviderId,
    pub name: String,
    pub provider_type: String,
    pub url: String,
    pub is_active: bool,
    pub total_vram_mb: i64,
    pub gpu_index: Option<i16>,
    pub server_id: Option<GpuServerId>,
    pub is_free_tier: bool,
    pub num_parallel: i16,
    pub status: String,
    pub registered_at: DateTime<Utc>,
    /// Masked API key shown in the management UI (e.g. `AIza...x1y2`). Gemini only.
    pub api_key_masked: Option<String>,
}

impl From<LlmProvider> for ProviderSummary {
    fn from(b: LlmProvider) -> Self {
        let provider_type = b.provider_type.as_str().to_string();
        let status = b.status.as_str().to_string();
        let api_key_masked = b.api_key_encrypted.as_deref().map(gemini_helpers::mask_api_key);
        Self {
            id: ProviderId::from_uuid(b.id),
            name: b.name,
            provider_type,
            url: b.url,
            is_active: b.is_active,
            total_vram_mb: b.total_vram_mb,
            gpu_index: b.gpu_index,
            server_id: b.server_id.map(GpuServerId::from_uuid),
            is_free_tier: b.is_free_tier,
            num_parallel: b.num_parallel,
            status,
            registered_at: b.registered_at,
            api_key_masked,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RegisterProviderResponse {
    pub id: ProviderId,
    pub status: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Fetch a provider by ID or return a structured error.
pub(super) async fn get_provider(state: &AppState, id: Uuid) -> Result<LlmProvider, AppError> {
    state
        .provider_registry
        .get(id)
        .await
        .map_err(|e| {
            tracing::error!(%id, error = %e, "failed to fetch provider");
            db_error(e)
        })?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /v1/providers/verify` — validate Ollama URL format, duplicate check, and connectivity.
pub async fn verify_provider(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<VerifyProviderRequest>,
) -> impl IntoResponse {
    let url = req.url.trim().to_string();

    if url.is_empty() {
        return AppError::BadRequest("url is required".into()).into_response();
    }

    if let Err(e) = validate_provider_url(&url) {
        return e.into_response();
    }

    // Duplicate check.
    let count_result: Result<(i64,), _> = sqlx::query_as(
        "SELECT COUNT(*) FROM llm_providers WHERE url = $1 AND provider_type = 'ollama'",
    )
    .bind(&url)
    .fetch_one(&state.pg_pool)
    .await;

    match count_result {
        Ok((c,)) if c > 0 => {
            return AppError::Conflict("a provider with this URL is already registered".into())
                .into_response();
        }
        Err(e) => return db_error(e).into_response(),
        _ => {}
    }

    // Connectivity check: GET {url}/api/version must succeed.
    let check_url = format!("{}/api/version", url.trim_end_matches('/'));
    match state
        .http_client
        .get(&check_url)
        .timeout(OLLAMA_HEALTH_CHECK_TIMEOUT)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            (StatusCode::OK, Json(serde_json::json!({"reachable": true}))).into_response()
        }
        Ok(r) => {
            tracing::warn!(url = %url, status = %r.status(), "Ollama verify probe returned unexpected status");
            AppError::BadGateway(
                format!("Ollama returned unexpected status {}", r.status()),
            )
            .into_response()
        }
        Err(e) => {
            tracing::warn!(url = %url, error = %e, "Ollama verify probe failed");
            AppError::BadGateway("Ollama is not reachable at the given URL".into())
                .into_response()
        }
    }
}

/// `POST /v1/providers` — register a new Ollama or Gemini provider.
///
/// Immediately runs a health check and sets the initial status.
pub async fn register_provider(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<RegisterProviderRequest>,
) -> impl IntoResponse {
    let Some(provider_type) = parse_provider_type(&req.provider_type) else {
        return AppError::BadRequest("provider_type must be 'ollama' or 'gemini'".into())
            .into_response();
    };

    // Validate required fields per provider type.
    match provider_type {
        ProviderType::Ollama => {
            let url = req.url.as_deref().unwrap_or("");
            if url.is_empty() {
                return AppError::BadRequest("url is required for ollama providers".into())
                    .into_response();
            }
            if let Err(e) = validate_provider_url(url) {
                return e.into_response();
            }
            // Reject duplicate Ollama URL.
            let count_result: Result<(i64,), _> = sqlx::query_as(
                "SELECT COUNT(*) FROM llm_providers WHERE url = $1 AND provider_type = 'ollama'",
            )
            .bind(url)
            .fetch_one(&state.pg_pool)
            .await;
            match count_result {
                Ok((count,)) if count > 0 => {
                    return AppError::Conflict(
                        "a provider with this URL is already registered".into(),
                    )
                    .into_response();
                }
                Err(e) => return db_error(e).into_response(),
                _ => {}
            }
        }
        ProviderType::Gemini => {
            if req.api_key.as_deref().unwrap_or("").is_empty() {
                return AppError::BadRequest("api_key is required for gemini providers".into())
                    .into_response();
            }
        }
    }

    let provider = LlmProvider {
        id: Uuid::now_v7(),
        name: req.name.clone(),
        provider_type,
        url: req.url.unwrap_or_default(),
        api_key_encrypted: req.api_key,
        is_active: true,
        total_vram_mb: req.total_vram_mb.unwrap_or(0),
        gpu_index: req.gpu_index,
        server_id: req.server_id.map(|s| s.0),
        is_free_tier: req.is_free_tier.unwrap_or(false),
        num_parallel: req.num_parallel.unwrap_or(4),
        status: LlmProviderStatus::Offline, // initial; overwritten by health check
        registered_at: Utc::now(),
    };

    // Health check before persisting.
    let initial_status = check_provider(&state.http_client, &provider).await;
    if matches!(provider_type, ProviderType::Ollama)
        && initial_status == LlmProviderStatus::Offline
    {
        return AppError::BadGateway("Ollama is not reachable at the given URL".into())
            .into_response();
    }
    let provider = LlmProvider {
        status: initial_status,
        ..provider
    };

    let registry = &state.provider_registry;
    if let Err(e) = registry.register(&provider).await {
        tracing::error!(error = %e, "failed to register provider");
        return db_error(e).into_response();
    }

    let status_str = initial_status.as_str();

    tracing::info!(
        id = %provider.id,
        provider_name = %provider.name,
        provider_type = %req.provider_type,
        status = %status_str,
        "provider registered"
    );

    let resource_type = provider.provider_type.resource_type();
    emit_audit(&state, &claims, "create", resource_type, &provider.id.to_string(), &provider.name,

        &format!("Provider '{}' registered (type: {}, initial_status: {})",
            provider.name, req.provider_type, status_str)).await;

    (
        StatusCode::CREATED,
        Json(RegisterProviderResponse {
            id: ProviderId::from_uuid(provider.id),
            status: status_str.to_string(),
        }),
    )
        .into_response()
}

/// `GET /v1/providers` — list registered providers with optional search/pagination/type filter.
#[derive(serde::Deserialize)]
pub struct ListProvidersParams {
    pub search: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
    /// Filter by provider type: "ollama" | "gemini". Omit for all types.
    pub provider_type: Option<String>,
}

pub async fn list_providers(
    State(state): State<AppState>,
    Query(params): Query<ListProvidersParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let offset = (page - 1) * limit;
    let provider_type = params.provider_type.as_deref();

    let (providers, total) = state.provider_registry.list_page(&search, provider_type, limit, offset).await
        .map_err(|e| { tracing::error!(error = %e, "failed to list providers"); db_error(e) })?;
    let summaries: Vec<ProviderSummary> = providers.into_iter().map(Into::into).collect();
    Ok(Json(serde_json::json!({
        "providers": summaries,
        "total": total,
        "page": page,
        "limit": limit,
    })))
}

/// `DELETE /v1/providers/{id}` — soft-delete (deactivate) a provider.
pub async fn delete_provider(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    let provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let name = provider.name.clone();
    let resource_type = provider.provider_type.resource_type();

    match &state.provider_registry.deactivate(id).await {
        Ok(()) => {
            emit_audit(&state, &claims, "delete", resource_type, &pid.to_string(), &name,
                &format!("Provider '{}' ({}) deactivated (soft-deleted, no longer routed)", name, pid)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!(%id, error = %e, "failed to deactivate provider");
            db_error(e).into_response()
        }
    }
}

/// `POST /v1/providers/{id}/healthcheck` — manually trigger a health check.
pub async fn healthcheck_provider(
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    let provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    let new_status = check_provider(&state.http_client, &provider).await;

    let registry = &state.provider_registry;
    if let Err(e) = registry.update_status(id, new_status).await {
        tracing::warn!(%id, error = %e, "failed to persist healthcheck result");
    }

    let status_str = new_status.as_str();

    (
        StatusCode::OK,
        Json(serde_json::json!({"id": pid.to_string(), "status": status_str})),
    )
        .into_response()
}

/// `PATCH /v1/providers/{id}` — update mutable fields of a provider.
///
/// All fields are optional; only provided (non-null) fields are applied.
/// Passing `api_key: ""` leaves the existing key unchanged.
pub async fn update_provider(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
    Json(req): Json<UpdateProviderRequest>,
) -> impl IntoResponse {
    let id = pid.0;
    let mut provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    let registry = &state.provider_registry;

    if req.name.trim().is_empty() {
        return AppError::BadRequest("name must not be empty".into()).into_response();
    }
    provider.name = req.name.trim().to_string();
    if let Some(ref url) = req.url {
        if !url.is_empty()
            && let Err(e) = validate_provider_url(url) {
                return e.into_response();
            }
        provider.url = url.clone();
    }
    // Empty / absent api_key = keep existing stored value.
    if let Some(key) = req.api_key.filter(|s| !s.is_empty()) {
        provider.api_key_encrypted = Some(key);
    }
    provider.total_vram_mb = req.total_vram_mb.unwrap_or(provider.total_vram_mb);
    provider.gpu_index = req.gpu_index;   // null clears the field
    provider.server_id = req.server_id.map(|s| s.0);  // null clears the field
    if let Some(v) = req.is_free_tier { provider.is_free_tier = v; }
    if let Some(v) = req.num_parallel { provider.num_parallel = v; }
    if let Some(v) = req.is_active { provider.is_active = v; }

    if let Err(e) = registry.update(&provider).await {
        tracing::error!(%id, error = %e, "update_provider: failed");
        return db_error(e).into_response();
    }

    let resource_type = provider.provider_type.resource_type();
    emit_audit(&state, &claims, "update", resource_type, &pid.to_string(), &provider.name,
        &format!("Provider '{}' ({}) configuration updated", provider.name, pid)).await;
    tracing::info!(%id, "provider updated");
    (StatusCode::OK, Json(ProviderSummary::from(provider))).into_response()
}

/// `GET /v1/providers/{id}/models` — list models available on a provider.
///
/// Returns the cached model list if available (TTL: 1 h).
/// On cache miss, fetches live from Ollama (`/api/tags`) or the Gemini models API,
/// stores the result in Valkey, and returns it.
pub async fn list_provider_models(
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    let _provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    let cache_key = models_cache_key(id);

    // ── Cache hit ────────────────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool
        && let Some(models) = load_models_cache(pool, &cache_key).await {
            return (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response();
        }

    // ── Cache miss: return empty — use POST /v1/providers/{id}/models/sync to populate ──
    (StatusCode::OK, Json(serde_json::json!({"models": []}))).into_response()
}

/// `GET /v1/providers/{id}/key` — return the decrypted API key for a Gemini provider.
///
/// Requires admin auth. Returns `{"key": "AIza..."}`.
pub async fn reveal_provider_key(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    let provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    match provider.api_key_encrypted {
        Some(key) => (StatusCode::OK, Json(serde_json::json!({"key": key}))).into_response(),
        None => {
            AppError::NotFound("no api key stored for this provider".into()).into_response()
        }
    }
}

/// `POST /v1/providers/{id}/models/sync` — force-refresh the model list from the provider.
///
/// For Ollama providers: ignores Valkey cache, fetches live, stores the fresh list.
/// For Gemini providers: returns 400 — use `POST /v1/gemini/models/sync` instead.
pub async fn sync_provider_models(
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    let provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    // Gemini model sync is global — direct the caller to the correct endpoint.
    if matches!(provider.provider_type, ProviderType::Gemini) {
        return AppError::BadRequest(
            "Use POST /v1/gemini/models/sync to sync Gemini models globally".into(),
        )
        .into_response();
    }

    match fetch_models_live(&state.http_client, &provider).await {
        Ok(models) => {
            let cache_key = models_cache_key(id);
            if let Some(ref pool) = state.valkey_pool {
                store_models_cache(pool, &cache_key, &models).await;
            }
            // Also persist to the global ollama_models table.
            if let Err(e) = state.ollama_model_repo.sync_provider_models(id, &models).await {
                tracing::warn!(%id, error = %e, "failed to persist ollama models to DB (non-fatal)");
            }
            // Upsert model selections (is_enabled defaults to true for new rows).
            if let Err(e) = state.model_selection_repo.upsert_models(id, &models).await {
                tracing::warn!(%id, error = %e, "failed to upsert model selections (non-fatal)");
            }
            tracing::info!(%id, count = models.len(), "model list synced");
            (
                StatusCode::OK,
                Json(serde_json::json!({"models": models, "synced": true})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(%id, error = %e, "model sync failed");
            AppError::ServiceUnavailable("provider sync failed".into()).into_response()
        }
    }
}

// ── Unified sync endpoints ──────────────────────────────────────────────────

/// `POST /v1/providers/{id}/sync` — unified sync for a single provider.
///
/// Combines health check + model sync + VRAM probing + LLM analysis.
pub async fn sync_single_provider(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    let provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    if !matches!(provider.provider_type, ProviderType::Ollama) {
        return AppError::BadRequest("sync is only supported for Ollama providers".into())
            .into_response();
    }

    let settings = state.capacity_settings_repo.get().await.unwrap_or_default();

    match crate::infrastructure::outbound::capacity::analyzer::sync_provider(
        &state.http_client,
        provider.id,
        &provider.name,
        &provider.url,
        provider.total_vram_mb,
        provider.num_parallel.max(1) as u32,
        &settings.analyzer_model,
        &*state.capacity_repo,
        &*state.vram_pool,
        state.valkey_pool.as_ref(),
        &*state.provider_registry,
        &*state.ollama_model_repo,
        &*state.model_selection_repo,
        &*state.vram_budget_repo,
        None,
    )
    .await
    {
        Ok(()) => {
            emit_audit(
                &state, &claims, "sync", "ollama_provider", &pid.to_string(),
                &provider.name, &format!("Provider '{}' synced", provider.name),
            )
            .await;
            (StatusCode::OK, Json(serde_json::json!({"synced": true}))).into_response()
        }
        Err(e) => {
            tracing::warn!(%id, error = %e, "sync_provider failed");
            AppError::ServiceUnavailable("provider sync failed".into()).into_response()
        }
    }
}

/// `POST /v1/providers/sync` — unified sync for all active Ollama providers.
pub async fn sync_all_providers_handler(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if state.sync_lock.available_permits() == 0 {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "message": "sync already in progress" })),
        )
            .into_response();
    }
    state.sync_trigger.notify_one();
    emit_audit(
        &state, &claims, "trigger", "capacity_settings", "capacity_settings",
        "provider_sync_all", "Manual sync all providers triggered",
    )
    .await;
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "message": "provider sync triggered" })),
    )
        .into_response()
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn provider_summary_from_llm_provider_ollama() {
        let b = LlmProvider {
            id: Uuid::now_v7(),
            name: "test-ollama".to_string(),
            provider_type: ProviderType::Ollama,
            url: "http://localhost:11434".to_string(),
            api_key_encrypted: None,
            is_active: true,
            total_vram_mb: 8192,
            gpu_index: Some(0),
            server_id: None,
                is_free_tier: false,
            num_parallel: 4,
            status: LlmProviderStatus::Online,
            registered_at: Utc::now(),
        };
        let s = ProviderSummary::from(b);
        assert_eq!(s.provider_type, "ollama");
        assert_eq!(s.status, "online");
        assert_eq!(s.url, "http://localhost:11434");
        assert!(s.is_active);
        assert_eq!(s.gpu_index, Some(0));
    }

    #[test]
    fn provider_summary_from_llm_provider_gemini() {
        let b = LlmProvider {
            id: Uuid::now_v7(),
            name: "gemini-pro".to_string(),
            provider_type: ProviderType::Gemini,
            url: String::new(),
            api_key_encrypted: Some("secret".to_string()),
            is_active: true,
            total_vram_mb: 0,
            gpu_index: None,
            server_id: None,
                is_free_tier: true,
            num_parallel: 4,
            status: LlmProviderStatus::Offline,
            registered_at: Utc::now(),
        };
        let s = ProviderSummary::from(b);
        assert_eq!(s.provider_type, "gemini");
        assert_eq!(s.status, "offline");
    }

    #[test]
    fn register_request_deserialization() {
        let json = r#"{"name":"local","provider_type":"ollama","url":"http://localhost:11434"}"#;
        let req: RegisterProviderRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "local");
        assert_eq!(req.provider_type, "ollama");
        assert_eq!(req.url.as_deref(), Some("http://localhost:11434"));
        assert!(req.api_key.is_none());
    }
}
