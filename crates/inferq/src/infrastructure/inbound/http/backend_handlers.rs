use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::LlmBackend;
use crate::domain::enums::{BackendType, LlmBackendStatus};
use crate::infrastructure::outbound::health_checker::check_backend;

use super::state::AppState;

// ── Request / Response DTOs ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterBackendRequest {
    /// Human-readable label.
    pub name: String,
    /// `"ollama"` or `"gemini"`.
    pub backend_type: String,
    /// Required for Ollama. E.g. `"http://192.168.1.10:11434"`.
    pub url: Option<String>,
    /// Required for Gemini. Stored as-is (PoC — no encryption).
    pub api_key: Option<String>,
    /// Reported GPU VRAM in MiB (informational, 0 if unknown).
    pub total_vram_mb: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct BackendSummary {
    pub id: Uuid,
    pub name: String,
    pub backend_type: String,
    pub url: String,
    pub is_active: bool,
    pub total_vram_mb: i64,
    pub status: String,
    pub registered_at: DateTime<Utc>,
}

impl From<LlmBackend> for BackendSummary {
    fn from(b: LlmBackend) -> Self {
        let backend_type = match b.backend_type {
            BackendType::Ollama => "ollama",
            BackendType::Gemini => "gemini",
        }
        .to_string();
        let status = match b.status {
            LlmBackendStatus::Online => "online",
            LlmBackendStatus::Offline => "offline",
            LlmBackendStatus::Degraded => "degraded",
        }
        .to_string();
        Self {
            id: b.id,
            name: b.name,
            backend_type,
            url: b.url,
            is_active: b.is_active,
            total_vram_mb: b.total_vram_mb,
            status,
            registered_at: b.registered_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RegisterBackendResponse {
    pub id: Uuid,
    pub status: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn backend_registry(state: &AppState) -> &Arc<dyn LlmBackendRegistry> {
    &state.backend_registry
}

fn parse_backend_type(s: &str) -> Option<BackendType> {
    match s.to_lowercase().as_str() {
        "ollama" => Some(BackendType::Ollama),
        "gemini" => Some(BackendType::Gemini),
        _ => None,
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /v1/backends` — register a new Ollama or Gemini backend.
///
/// Immediately runs a health check and sets the initial status.
pub async fn register_backend(
    State(state): State<AppState>,
    Json(req): Json<RegisterBackendRequest>,
) -> impl IntoResponse {
    let Some(backend_type) = parse_backend_type(&req.backend_type) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "backend_type must be 'ollama' or 'gemini'"})),
        )
            .into_response();
    };

    // Validate required fields per backend type.
    match backend_type {
        BackendType::Ollama => {
            if req.url.as_deref().unwrap_or("").is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "url is required for ollama backends"})),
                )
                    .into_response();
            }
        }
        BackendType::Gemini => {
            if req.api_key.as_deref().unwrap_or("").is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "api_key is required for gemini backends"})),
                )
                    .into_response();
            }
        }
    }

    let backend = LlmBackend {
        id: Uuid::now_v7(),
        name: req.name.clone(),
        backend_type: backend_type.clone(),
        url: req.url.unwrap_or_default(),
        api_key_encrypted: req.api_key,
        is_active: true,
        total_vram_mb: req.total_vram_mb.unwrap_or(0),
        status: LlmBackendStatus::Offline, // initial; overwritten by health check
        registered_at: Utc::now(),
    };

    // Health check before persisting.
    let client = reqwest::Client::new();
    let initial_status = check_backend(&client, &backend).await;
    let backend = LlmBackend {
        status: initial_status.clone(),
        ..backend
    };

    let registry = backend_registry(&state);
    if let Err(e) = registry.register(&backend).await {
        tracing::error!("failed to register backend: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    let status_str = match initial_status {
        LlmBackendStatus::Online => "online",
        LlmBackendStatus::Offline => "offline",
        LlmBackendStatus::Degraded => "degraded",
    };

    tracing::info!(
        id = %backend.id,
        name = %backend.name,
        backend_type = %req.backend_type,
        status = %status_str,
        "backend registered"
    );

    (
        StatusCode::CREATED,
        Json(RegisterBackendResponse {
            id: backend.id,
            status: status_str.to_string(),
        }),
    )
        .into_response()
}

/// `GET /v1/backends` — list all registered backends.
pub async fn list_backends(State(state): State<AppState>) -> impl IntoResponse {
    match backend_registry(&state).list_all().await {
        Ok(backends) => {
            let summaries: Vec<BackendSummary> = backends.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(summaries)).into_response()
        }
        Err(e) => {
            tracing::error!("failed to list backends: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `DELETE /v1/backends/{id}` — soft-delete (deactivate) a backend.
pub async fn delete_backend(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match backend_registry(&state).deactivate(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(%id, "failed to deactivate backend: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `POST /v1/backends/{id}/healthcheck` — manually trigger a health check.
pub async fn healthcheck_backend(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let registry = backend_registry(&state);

    let backend = match registry.get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "backend not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "failed to fetch backend: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    let client = reqwest::Client::new();
    let new_status = check_backend(&client, &backend).await;

    if let Err(e) = registry.update_status(id, new_status.clone()).await {
        tracing::warn!(%id, "failed to persist healthcheck result: {e}");
    }

    let status_str = match new_status {
        LlmBackendStatus::Online => "online",
        LlmBackendStatus::Offline => "offline",
        LlmBackendStatus::Degraded => "degraded",
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({"id": id, "status": status_str})),
    )
        .into_response()
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_summary_from_llm_backend_ollama() {
        let b = LlmBackend {
            id: Uuid::now_v7(),
            name: "test-ollama".to_string(),
            backend_type: BackendType::Ollama,
            url: "http://localhost:11434".to_string(),
            api_key_encrypted: None,
            is_active: true,
            total_vram_mb: 8192,
            status: LlmBackendStatus::Online,
            registered_at: Utc::now(),
        };
        let s = BackendSummary::from(b);
        assert_eq!(s.backend_type, "ollama");
        assert_eq!(s.status, "online");
        assert_eq!(s.url, "http://localhost:11434");
        assert!(s.is_active);
    }

    #[test]
    fn backend_summary_from_llm_backend_gemini() {
        let b = LlmBackend {
            id: Uuid::now_v7(),
            name: "gemini-pro".to_string(),
            backend_type: BackendType::Gemini,
            url: String::new(),
            api_key_encrypted: Some("secret".to_string()),
            is_active: true,
            total_vram_mb: 0,
            status: LlmBackendStatus::Offline,
            registered_at: Utc::now(),
        };
        let s = BackendSummary::from(b);
        assert_eq!(s.backend_type, "gemini");
        assert_eq!(s.status, "offline");
    }

    #[test]
    fn parse_backend_type_case_insensitive() {
        assert_eq!(parse_backend_type("Ollama"), Some(BackendType::Ollama));
        assert_eq!(parse_backend_type("GEMINI"), Some(BackendType::Gemini));
        assert_eq!(parse_backend_type("unknown"), None);
    }

    #[test]
    fn register_request_deserialization() {
        let json = r#"{"name":"local","backend_type":"ollama","url":"http://localhost:11434"}"#;
        let req: RegisterBackendRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "local");
        assert_eq!(req.backend_type, "ollama");
        assert_eq!(req.url.as_deref(), Some("http://localhost:11434"));
        assert!(req.api_key.is_none());
    }
}

/// `GET /v1/backends/{id}/models` — list models available on a backend.
///
/// For Ollama: calls `GET {url}/api/tags`.
/// For Gemini: returns a static list of supported model names.
pub async fn list_backend_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let registry = backend_registry(&state);
    let backend = match registry.get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "backend not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "failed to fetch backend: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    match backend.backend_type {
        BackendType::Ollama => {
            let url = format!("{}/api/tags", backend.url.trim_end_matches('/'));
            let client = reqwest::Client::new();
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<serde_json::Value>().await {
                        Ok(json) => {
                            let models: Vec<String> = json["models"]
                                .as_array()
                                .unwrap_or(&vec![])
                                .iter()
                                .filter_map(|m| m["name"].as_str().map(String::from))
                                .collect();
                            (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response()
                        }
                        Err(e) => {
                            tracing::error!(%id, "failed to parse ollama tags: {e}");
                            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": "failed to parse ollama response"}))).into_response()
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": format!("ollama returned {status}")}))).into_response()
                }
                Err(e) => {
                    tracing::error!(%id, "failed to reach ollama: {e}");
                    (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": "cannot reach ollama"}))).into_response()
                }
            }
        }
        BackendType::Gemini => {
            // Return commonly available Gemini models
            let models = vec![
                "gemini-2.0-flash",
                "gemini-2.0-flash-lite",
                "gemini-1.5-flash",
                "gemini-1.5-pro",
            ];
            (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response()
        }
    }
}
