use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::enums::{BackendType, LlmBackendStatus};
use crate::infrastructure::outbound::health_checker::check_backend;

use super::state::AppState;

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

// ── Helpers ────────────────────────────────────────────────────────────────────

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

/// Fetch the list of Gemini models that support `generateContent` using `api_key`.
async fn fetch_gemini_models(api_key: &str) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={api_key}"
    );

    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach gemini api: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("gemini api returned error: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse gemini response: {e}"))?;

    let models = json["models"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter(|m| {
            m["supportedGenerationMethods"]
                .as_array()
                .map(|methods| {
                    methods
                        .iter()
                        .any(|method| method.as_str() == Some("generateContent"))
                })
                .unwrap_or(false)
        })
        .filter_map(|m| {
            m["name"]
                .as_str()
                .map(|s| s.strip_prefix("models/").unwrap_or(s).to_string())
        })
        .collect();

    Ok(models)
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/gemini/sync-config` — return the masked admin API key (or null).
pub async fn get_sync_config(State(state): State<AppState>) -> impl IntoResponse {
    match state.gemini_sync_config_repo.get_api_key().await {
        Ok(key) => {
            let masked = key.as_deref().map(mask_key);
            (StatusCode::OK, Json(SyncConfigResponse { api_key_masked: masked })).into_response()
        }
        Err(e) => {
            tracing::error!("get_sync_config: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `PUT /v1/gemini/sync-config` — store (or replace) the admin API key.
pub async fn set_sync_config(
    State(state): State<AppState>,
    Json(req): Json<SetSyncConfigRequest>,
) -> impl IntoResponse {
    if req.api_key.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "api_key must not be empty"})),
        )
            .into_response();
    }

    match state.gemini_sync_config_repo.set_api_key(req.api_key.trim()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("set_sync_config: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `POST /v1/gemini/models/sync` — fetch the global Gemini model list and persist it.
///
/// Uses the stored admin API key. Returns `400` if no key is configured.
pub async fn sync_models(State(state): State<AppState>) -> impl IntoResponse {
    let api_key = match state.gemini_sync_config_repo.get_api_key().await {
        Ok(Some(k)) => k,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "No admin API key configured. Use PUT /v1/gemini/sync-config first."
                })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("sync_models: failed to fetch config: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    let models = match fetch_gemini_models(&api_key).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("sync_models: gemini api error: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    if let Err(e) = state.gemini_model_repo.sync_models(&models).await {
        tracing::error!("sync_models: failed to persist: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    let count = models.len();
    tracing::info!(count, "global gemini model list synced");
    (StatusCode::OK, Json(SyncModelsResponse { models, count })).into_response()
}

// ── Status sync ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GeminiStatusResult {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GeminiSyncStatusResponse {
    pub synced_at: DateTime<Utc>,
    pub results: Vec<GeminiStatusResult>,
}

/// `POST /v1/gemini/sync-status` — check all active Gemini backends and update their status.
///
/// Runs synchronously (fast — just one lightweight API call per backend).
/// Returns the updated status for each backend.
pub async fn sync_status(State(state): State<AppState>) -> impl IntoResponse {
    let backends = match state.backend_registry.list_all().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("gemini sync_status: failed to list backends: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    let gemini_active: Vec<_> = backends
        .into_iter()
        .filter(|b| b.is_active && matches!(b.backend_type, BackendType::Gemini))
        .collect();

    let client = reqwest::Client::new();
    let mut results = Vec::with_capacity(gemini_active.len());

    for backend in gemini_active {
        let new_status = check_backend(&client, &backend).await;
        let status_str = match new_status {
            LlmBackendStatus::Online => "online",
            LlmBackendStatus::Offline => "offline",
            LlmBackendStatus::Degraded => "degraded",
        }
        .to_string();

        if let Err(e) = state.backend_registry.update_status(backend.id, new_status).await {
            tracing::warn!(backend_id = %backend.id, "gemini sync_status: failed to persist status: {e}");
        }

        results.push(GeminiStatusResult {
            id: backend.id,
            name: backend.name,
            status: status_str,
            error: None,
        });
    }

    tracing::info!(count = results.len(), "gemini status sync completed");
    (
        StatusCode::OK,
        Json(GeminiSyncStatusResponse {
            synced_at: Utc::now(),
            results,
        }),
    )
        .into_response()
}

/// `GET /v1/gemini/models` — list the global Gemini model pool.
pub async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    match state.gemini_model_repo.list().await {
        Ok(rows) => {
            let dtos: Vec<GeminiModelDto> = rows
                .into_iter()
                .map(|m| GeminiModelDto {
                    model_name: m.model_name,
                    synced_at: m.synced_at,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"models": dtos}))).into_response()
        }
        Err(e) => {
            tracing::error!("list_models: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}
