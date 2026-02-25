use std::sync::Arc;

use anyhow::Result;
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

// ── Model cache helpers ─────────────────────────────────────────────────────────

/// Valkey TTL for the model list cache (1 hour).
const MODELS_CACHE_TTL: i64 = 3600;

fn models_cache_key(id: Uuid) -> String {
    format!("inferq:models:{id}")
}

/// Fetch the list of available models directly from the backend (bypasses cache).
///
/// * Ollama → `GET {url}/api/tags`
/// * Gemini → `GET https://generativelanguage.googleapis.com/v1beta/models?key={api_key}`
///   filtered to models that support `generateContent`.
async fn fetch_models_live(backend: &LlmBackend) -> Result<Vec<String>> {
    let client = reqwest::Client::new();

    match backend.backend_type {
        BackendType::Ollama => {
            let url = format!("{}/api/tags", backend.url.trim_end_matches('/'));
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
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|m| m["name"].as_str().map(String::from))
                .collect();

            Ok(models)
        }

        BackendType::Gemini => {
            let api_key = backend
                .api_key_encrypted
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("gemini backend has no api key stored"))?;

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
    }
}

/// Write models to the Valkey cache (fire-and-forget; errors are logged, not surfaced).
async fn store_models_cache(pool: &fred::clients::RedisPool, key: &str, models: &[String]) {
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
        tracing::warn!("failed to cache model list: {e}");
    }
}

/// Read models from the Valkey cache. Returns `None` on miss or error.
async fn load_models_cache(pool: &fred::clients::RedisPool, key: &str) -> Option<Vec<String>> {
    use fred::prelude::*;

    let cached: Option<String> = pool.get(key).await.unwrap_or(None);
    let json_str = cached?;
    serde_json::from_str::<Vec<String>>(&json_str).ok()
}

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
    /// GPU VRAM capacity in MiB (manual). 0 = unknown.
    pub total_vram_mb: Option<i64>,
    /// GPU index on the host (0-based). For metric correlation.
    pub gpu_index: Option<i16>,
    /// FK → gpu_servers. Optional; Gemini backends leave this null.
    pub server_id: Option<Uuid>,
    /// inferq-agent URL (Phase 2, reserved). E.g. `"http://192.168.1.10:9091"`.
    pub agent_url: Option<String>,
}

/// Update request for `PATCH /v1/backends/{id}`.
///
/// The web UI pre-fills all current values before submission, so every field
/// is always present.  `gpu_index` / `server_id` = `null` explicitly clears them.
/// `api_key` = `null` or empty string keeps the existing stored key.
#[derive(Debug, Deserialize)]
pub struct UpdateBackendRequest {
    pub name: String,
    /// Ollama URL. Leave empty for Gemini.
    pub url: Option<String>,
    /// Replace the stored key when non-empty; otherwise keep existing.
    pub api_key: Option<String>,
    pub total_vram_mb: Option<i64>,
    pub gpu_index: Option<i16>,
    pub server_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct BackendSummary {
    pub id: Uuid,
    pub name: String,
    pub backend_type: String,
    pub url: String,
    pub is_active: bool,
    pub total_vram_mb: i64,
    pub gpu_index: Option<i16>,
    pub server_id: Option<Uuid>,
    pub agent_url: Option<String>,
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
            gpu_index: b.gpu_index,
            server_id: b.server_id,
            agent_url: b.agent_url,
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
        gpu_index: req.gpu_index,
        server_id: req.server_id,
        agent_url: req.agent_url.filter(|s| !s.is_empty()),
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

/// `PATCH /v1/backends/{id}` — update mutable fields of a backend.
///
/// All fields are optional; only provided (non-null) fields are applied.
/// Passing `api_key: ""` leaves the existing key unchanged.
pub async fn update_backend(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateBackendRequest>,
) -> impl IntoResponse {
    let registry = backend_registry(&state);

    let mut backend = match registry.get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "backend not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "update_backend: db error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must not be empty"})),
        )
            .into_response();
    }
    backend.name = req.name.trim().to_string();
    if let Some(url) = req.url {
        backend.url = url;
    }
    // Empty / absent api_key = keep existing stored value.
    if let Some(key) = req.api_key.filter(|s| !s.is_empty()) {
        backend.api_key_encrypted = Some(key);
    }
    backend.total_vram_mb = req.total_vram_mb.unwrap_or(backend.total_vram_mb);
    backend.gpu_index = req.gpu_index;   // null clears the field
    backend.server_id = req.server_id;  // null clears the field

    if let Err(e) = registry.update(&backend).await {
        tracing::error!(%id, "update_backend: failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    tracing::info!(%id, "backend updated");
    (StatusCode::OK, Json(BackendSummary::from(backend))).into_response()
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
            gpu_index: Some(0),
            server_id: None,
            agent_url: None,
            status: LlmBackendStatus::Online,
            registered_at: Utc::now(),
        };
        let s = BackendSummary::from(b);
        assert_eq!(s.backend_type, "ollama");
        assert_eq!(s.status, "online");
        assert_eq!(s.url, "http://localhost:11434");
        assert!(s.is_active);
        assert_eq!(s.gpu_index, Some(0));
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
            gpu_index: None,
            server_id: None,
            agent_url: None,
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
/// Returns the cached model list if available (TTL: 1 h).
/// On cache miss, fetches live from Ollama (`/api/tags`) or the Gemini models API,
/// stores the result in Valkey, and returns it.
pub async fn list_backend_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let backend = match backend_registry(&state).get(id).await {
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

    let cache_key = models_cache_key(id);

    // ── Cache hit ────────────────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool {
        if let Some(models) = load_models_cache(pool, &cache_key).await {
            return (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response();
        }
    }

    // ── Cache miss: fetch live ───────────────────────────────────────────────────
    match fetch_models_live(&backend).await {
        Ok(models) => {
            if let Some(ref pool) = state.valkey_pool {
                store_models_cache(pool, &cache_key, &models).await;
            }
            (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response()
        }
        Err(e) => {
            tracing::error!(%id, "failed to fetch models: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// `POST /v1/backends/{id}/models/sync` — force-refresh the model list from the backend.
///
/// Ignores the Valkey cache, fetches live, stores the fresh list, and returns it.
pub async fn sync_backend_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let backend = match backend_registry(&state).get(id).await {
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

    match fetch_models_live(&backend).await {
        Ok(models) => {
            let cache_key = models_cache_key(id);
            if let Some(ref pool) = state.valkey_pool {
                store_models_cache(pool, &cache_key, &models).await;
            }
            tracing::info!(%id, count = models.len(), "model list synced");
            (
                StatusCode::OK,
                Json(serde_json::json!({"models": models, "synced": true})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(%id, "model sync failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}
