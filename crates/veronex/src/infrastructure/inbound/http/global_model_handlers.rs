use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSettingsManage;
use super::error::AppError;
use super::state::AppState;

#[derive(Serialize)]
pub struct GlobalModelSettingResponse {
    pub model_name: String,
    pub is_enabled: bool,
}

#[derive(Deserialize)]
pub struct SetEnabledBody {
    pub is_enabled: bool,
}

/// GET /v1/models/global-settings — List all global model settings.
pub async fn list_global_model_settings(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
) -> Result<Json<Vec<GlobalModelSettingResponse>>, AppError> {
    let settings = state.global_model_settings_repo.list().await?;
    Ok(Json(settings.into_iter().map(|s| GlobalModelSettingResponse {
        model_name: s.model_name,
        is_enabled: s.is_enabled,
    }).collect()))
}

/// GET /v1/models/global-disabled — List globally disabled model names.
pub async fn list_global_disabled_models(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, AppError> {
    let disabled = state.global_model_settings_repo.list_disabled().await?;
    Ok(Json(disabled))
}

/// PATCH /v1/models/global-settings/{model_name} — Set global enable/disable.
pub async fn set_global_model_enabled(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    Json(body): Json<SetEnabledBody>,
) -> Result<Json<GlobalModelSettingResponse>, AppError> {
    state.global_model_settings_repo.set_enabled(&model_name, body.is_enabled).await?;
    Ok(Json(GlobalModelSettingResponse {
        model_name,
        is_enabled: body.is_enabled,
    }))
}
