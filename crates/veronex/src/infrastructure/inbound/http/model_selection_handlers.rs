use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::enums::ProviderType;
use crate::domain::value_objects::ProviderId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::inbound::http::provider_handlers::get_provider;

use super::audit_helpers::emit_audit;
use super::error::db_error;
use super::state::AppState;

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SelectedModelDto {
    model_name: String,
    is_enabled: bool,
    synced_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SetModelEnabledRequest {
    pub is_enabled: bool,
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/providers/{id}/selected-models` — list models with per-provider enabled state.
///
/// **Ollama**: merges per-provider `ollama_models` with `provider_selected_models`.
///   New models default to `is_enabled = true`.
/// **Gemini**: merges the global `gemini_models` pool with `provider_selected_models`.
///   New models default to `is_enabled = false`.
pub async fn list_selected_models(
    State(state): State<AppState>,
    Path(pid): Path<ProviderId>,
) -> impl IntoResponse {
    let id = pid.0;
    // Resolve the provider to branch by type.
    let provider = match get_provider(&state, id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    // Per-provider selections (enabled/disabled overrides).
    let selections = match state.model_selection_repo.list(id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%id, "list_selected_models: failed to list selections: {e}");
            return db_error(e).into_response();
        }
    };
    let sel_map: HashMap<String, bool> = selections
        .into_iter()
        .map(|s| (s.model_name, s.is_enabled))
        .collect();

    match provider.provider_type {
        ProviderType::Ollama => {
            // Use per-provider synced model list; default is_enabled = true.
            let models = match state.ollama_model_repo.models_for_provider(id).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!(%id, "list_selected_models: failed to list ollama models: {e}");
                    return db_error(e).into_response();
                }
            };
            let dtos: Vec<SelectedModelDto> = models
                .into_iter()
                .map(|model_name| {
                    let is_enabled = sel_map.get(&model_name).copied().unwrap_or(true);
                    SelectedModelDto {
                        model_name,
                        is_enabled,
                        synced_at: Utc::now(),
                    }
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"models": dtos}))).into_response()
        }

        ProviderType::Gemini => {
            // Global model pool; default is_enabled = false.
            let global = match state.gemini_model_repo.list().await {
                Ok(g) => g,
                Err(e) => {
                    tracing::error!(%id, "list_selected_models: failed to list global models: {e}");
                    return db_error(e).into_response();
                }
            };
            let dtos: Vec<SelectedModelDto> = global
                .into_iter()
                .map(|m| {
                    let is_enabled = sel_map.get(&m.model_name).copied().unwrap_or(false);
                    SelectedModelDto {
                        model_name: m.model_name,
                        is_enabled,
                        synced_at: m.synced_at,
                    }
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"models": dtos}))).into_response()
        }
    }
}

/// `PATCH /v1/providers/{id}/selected-models/{model_name}` — toggle a model's enabled state.
pub async fn set_model_enabled(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path((pid, model_name)): Path<(ProviderId, String)>,
    Json(req): Json<SetModelEnabledRequest>,
) -> impl IntoResponse {
    let id = pid.0;
    match state
        .model_selection_repo
        .set_enabled(id, &model_name, req.is_enabled)
        .await
    {
        Ok(()) => {
            let action = if req.is_enabled { "enable" } else { "disable" };
            emit_audit(&state, &claims, action, "provider_model_selection",
                &id.to_string(), &model_name,
                &format!("provider {id} model {model_name} set is_enabled={}", req.is_enabled)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!(%id, %model_name, "set_model_enabled: {e}");
            db_error(e).into_response()
        }
    }
}
