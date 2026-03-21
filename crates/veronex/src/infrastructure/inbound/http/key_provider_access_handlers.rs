use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSettingsManage;
use super::error::AppError;
use super::handlers::parse_uuid;
use super::state::AppState;

#[derive(Serialize)]
pub struct ProviderAccessEntry {
    pub provider_id: String,
    pub is_allowed: bool,
}

#[derive(Deserialize)]
pub struct SetAccessBody {
    pub is_allowed: bool,
}

/// GET /v1/keys/{key_id}/providers — List provider access rules for a key.
pub async fn list_key_provider_access(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path(key_id): Path<String>,
) -> Result<Json<Vec<ProviderAccessEntry>>, AppError> {
    let uuid = parse_uuid(&key_id)?;
    let rows = state.api_key_provider_access_repo.list(uuid).await?;
    Ok(Json(rows.into_iter().map(|(pid, allowed)| ProviderAccessEntry {
        provider_id: pid.to_string(),
        is_allowed: allowed,
    }).collect()))
}

/// PATCH /v1/keys/{key_id}/providers/{provider_id} — Set provider access for a key.
pub async fn set_key_provider_access(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path((key_id, provider_id)): Path<(String, String)>,
    Json(body): Json<SetAccessBody>,
) -> Result<Json<ProviderAccessEntry>, AppError> {
    let key_uuid = parse_uuid(&key_id)?;
    let provider_uuid = parse_uuid(&provider_id)?;
    state.api_key_provider_access_repo.set_access(key_uuid, provider_uuid, body.is_allowed).await?;
    Ok(Json(ProviderAccessEntry {
        provider_id,
        is_allowed: body.is_allowed,
    }))
}
