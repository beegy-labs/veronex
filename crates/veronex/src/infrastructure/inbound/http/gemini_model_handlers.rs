use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::enums::ProviderType;
use crate::domain::value_objects::ProviderId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::outbound::health_checker::check_provider;

use super::audit_helpers::emit_audit;
use super::error::{AppError, db_error};
use super::gemini_helpers;
use super::state::AppState;

type HandlerResult<T> = Result<T, AppError>;

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncConfigResponse {
    /// Masked admin API key, e.g. `"AIza...x1y2"`, or `null` if not yet set.
    pub api_key_masked: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetSyncConfigRequest {
    pub api_key: String,
}

#[derive(Debug, Serialize)]
pub struct GeminiModelDto {
    pub model_name: String,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SyncModelsResponse {
    pub models: Vec<String>,
    pub count: usize,
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/gemini/sync-config` — return the masked admin API key (or null).
pub async fn get_sync_config(RequireProviderManage(_claims): RequireProviderManage, State(state): State<AppState>) -> HandlerResult<Json<SyncConfigResponse>> {
    let key = state.gemini_sync_config_repo.get_api_key().await.map_err(db_error)?;
    let masked = key.as_deref().map(gemini_helpers::mask_api_key);
    Ok(Json(SyncConfigResponse { api_key_masked: masked }))
}

/// `PUT /v1/gemini/sync-config` — store (or replace) the admin API key.
pub async fn set_sync_config(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<SetSyncConfigRequest>,
) -> HandlerResult<StatusCode> {
    if req.api_key.trim().is_empty() {
        return Err(AppError::BadRequest("api_key must not be empty".into()));
    }

    state.gemini_sync_config_repo.set_api_key(req.api_key.trim()).await.map_err(db_error)?;

    emit_audit(&state, &claims, "update", "gemini_provider", "gemini_sync_config", "gemini_sync_config",
        "Gemini admin API key replaced (used for global model list sync)").await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/gemini/models/sync` — fetch the global Gemini model list and persist it.
///
/// Uses the stored admin API key. Returns `400` if no key is configured.
pub async fn sync_models(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
) -> HandlerResult<Json<SyncModelsResponse>> {
    let api_key = match state.gemini_sync_config_repo.get_api_key().await {
        Ok(Some(k)) => k,
        Ok(None) => {
            return Err(AppError::BadRequest(
                "No admin API key configured. Use PUT /v1/gemini/sync-config first.".into(),
            ));
        }
        Err(e) => return Err(db_error(e)),
    };

    let models = gemini_helpers::fetch_gemini_models(&state.http_client, &api_key)
        .await
        .map_err(|e| {
            tracing::error!("sync_models: gemini api error: {e}");
            AppError::BadGateway("Gemini API request failed".into())
        })?;

    state.gemini_model_repo.sync_models(&models).await.map_err(db_error)?;

    let count = models.len();
    tracing::info!(count, "global gemini model list synced");
    emit_audit(&state, &claims, "sync", "gemini_provider", &format!("{count} models"), "gemini_models",
        &format!("Global Gemini model list synced from API: {count} models discovered")).await;
    Ok(Json(SyncModelsResponse { models, count }))
}

// ── Status sync ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GeminiStatusResult {
    pub id: ProviderId,
    pub name: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GeminiSyncStatusResponse {
    pub synced_at: DateTime<Utc>,
    pub results: Vec<GeminiStatusResult>,
}

/// `POST /v1/gemini/sync-status` — check all active Gemini providers and update their status.
///
/// Runs synchronously (fast — just one lightweight API call per provider).
/// Returns the updated status for each provider.
pub async fn sync_status(RequireProviderManage(claims): RequireProviderManage, State(state): State<AppState>) -> HandlerResult<Json<GeminiSyncStatusResponse>> {
    let providers = state.provider_registry.list_all().await.map_err(db_error)?;

    let gemini_active: Vec<_> = providers
        .into_iter()
        .filter(|p| matches!(p.provider_type, ProviderType::Gemini))
        .collect();

    let mut results = Vec::with_capacity(gemini_active.len());

    for provider in gemini_active {
        let new_status = check_provider(&state.http_client, &provider).await;
        let status_str = new_status.as_str().to_string();

        if let Err(e) = state.provider_registry.update_status(provider.id, new_status).await {
            tracing::warn!(provider_id = %provider.id, "gemini sync_status: failed to persist status: {e}");
        }

        results.push(GeminiStatusResult {
            id: ProviderId::from_uuid(provider.id),
            name: provider.name,
            status: status_str,
            error: None,
        });
    }

    tracing::info!(count = results.len(), "gemini status sync completed");

    emit_audit(&state, &claims, "trigger", "gemini_sync_status",
        "gemini", "gemini",
        &format!("Gemini status sync ran for {} providers", results.len())).await;

    Ok(Json(GeminiSyncStatusResponse {
        synced_at: Utc::now(),
        results,
    }))
}

/// `GET /v1/gemini/models` — list the global Gemini model pool.
pub async fn list_models(RequireProviderManage(_claims): RequireProviderManage, State(state): State<AppState>) -> HandlerResult<impl IntoResponse> {
    let rows = state.gemini_model_repo.list().await.map_err(db_error)?;
    let dtos: Vec<GeminiModelDto> = rows
        .into_iter()
        .map(|m| GeminiModelDto {
            model_name: m.model_name,
            synced_at: m.synced_at,
        })
        .collect();
    Ok(Json(serde_json::json!({"models": dtos})))
}
