//! Phase 3 — admin endpoints for `system_settings`.
//!
//! Generic key/value over [`SystemSettingsRepository`]. The repo bakes in
//! defaults for unset canonical keys so a fresh install behaves sanely
//! before the operator visits the admin UI; this handler exposes the
//! same defaults via `GET /:key` (returns the seed value when the row
//! is absent rather than 404).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::application::ports::outbound::system_settings_repository::default_for;
use crate::infrastructure::inbound::http::handlers::internal_json_error;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::inbound::http::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpsertSettingRequest {
    pub value: String,
    #[serde(default)]
    pub description: Option<String>,
}

fn not_wired() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "system_settings_not_wired",
            "message": "SystemSettingsRepository is not configured.",
        })),
    )
        .into_response()
}

/// `GET /v1/admin/settings` — list every persisted row plus seed values
/// for canonical keys that haven't been written yet.
#[instrument(skip_all)]
pub async fn list_settings(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(repo) = state.system_settings_repo.as_ref() else {
        return not_wired();
    };
    match repo.list_all().await {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|s| {
                    json!({
                        "key":         s.key,
                        "value":       s.value,
                        "description": s.description,
                        "updated_at":  s.updated_at,
                        "updated_by":  s.updated_by,
                        "is_default":  false,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "settings": items, "total": items.len() })))
                .into_response()
        }
        Err(e) => internal_json_error("list_failed", &e),
    }
}

/// `GET /v1/admin/settings/{key}` — single key. Falls back to the
/// canonical default when the row is absent so callers always get a
/// usable value.
#[instrument(skip_all)]
pub async fn get_setting(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let Some(repo) = state.system_settings_repo.as_ref() else {
        return not_wired();
    };
    match repo.get(&key).await {
        Ok(Some(s)) => (
            StatusCode::OK,
            Json(json!({
                "key":         s.key,
                "value":       s.value,
                "description": s.description,
                "updated_at":  s.updated_at,
                "updated_by":  s.updated_by,
                "is_default":  false,
            })),
        )
            .into_response(),
        Ok(None) => match default_for(&key) {
            Some(v) => (
                StatusCode::OK,
                Json(json!({
                    "key":         key,
                    "value":       v,
                    "description": null,
                    "updated_at":  null,
                    "updated_by":  null,
                    "is_default":  true,
                })),
            )
                .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "key_not_found", "key": key })),
            )
                .into_response(),
        },
        Err(e) => internal_json_error("get_failed", &e),
    }
}

/// `PUT /v1/admin/settings/{key}` — upsert.
#[instrument(skip_all)]
pub async fn upsert_setting(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<UpsertSettingRequest>,
) -> impl IntoResponse {
    let Some(repo) = state.system_settings_repo.as_ref() else {
        return not_wired();
    };
    match repo
        .upsert(&key, &req.value, req.description.as_deref(), Some(claims.sub))
        .await
    {
        Ok(_) => (StatusCode::OK, Json(json!({ "ok": true, "key": key }))).into_response(),
        Err(e) => internal_json_error("upsert_failed", &e),
    }
}

/// `DELETE /v1/admin/settings/{key}`
#[instrument(skip_all)]
pub async fn delete_setting(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let Some(repo) = state.system_settings_repo.as_ref() else {
        return not_wired();
    };
    match repo.delete(&key).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "key_not_found", "key": key })),
        )
            .into_response(),
        Err(e) => internal_json_error("delete_failed", &e),
    }
}
