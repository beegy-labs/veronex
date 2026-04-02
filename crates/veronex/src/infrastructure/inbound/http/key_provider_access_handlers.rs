use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::domain::value_objects::{ApiKeyId, ProviderId};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSettingsManage;
use super::error::AppError;
use super::state::AppState;

#[derive(Serialize)]
pub struct ProviderAccessEntry {
    pub provider_id: ProviderId,
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
    Path(kid): Path<ApiKeyId>,
) -> Result<Json<Vec<ProviderAccessEntry>>, AppError> {
    let rows = state.api_key_provider_access_repo.list(kid.0).await?;
    Ok(Json(rows.into_iter().map(|(pid, allowed)| ProviderAccessEntry {
        provider_id: ProviderId::from_uuid(pid),
        is_allowed: allowed,
    }).collect()))
}

/// PATCH /v1/keys/{key_id}/providers/{provider_id} — Set provider access for a key.
pub async fn set_key_provider_access(
    RequireSettingsManage(_): RequireSettingsManage,
    State(state): State<AppState>,
    Path((kid, pid)): Path<(ApiKeyId, ProviderId)>,
    Json(body): Json<SetAccessBody>,
) -> Result<Json<ProviderAccessEntry>, AppError> {
    state.api_key_provider_access_repo.set_access(kid.0, pid.0, body.is_allowed).await?;
    Ok(Json(ProviderAccessEntry {
        provider_id: pid,
        is_allowed: body.is_allowed,
    }))
}
