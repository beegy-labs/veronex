use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::audit_port::AuditEvent;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{LlmProviderStatus, ProviderType};
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;
use crate::infrastructure::outbound::health_checker::check_provider;

use super::state::AppState;

async fn emit_audit(
    state: &AppState,
    actor: &Claims,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    resource_name: &str,
    details: &str,
) {
    if let Some(ref port) = state.audit_port {
        port.record(AuditEvent {
            event_time: Utc::now(),
            account_id: actor.sub,
            account_name: actor.sub.to_string(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            resource_name: resource_name.to_string(),
            ip_address: None,
            details: Some(details.to_string()),
        })
        .await;
    }
}

// ── Model cache helpers ─────────────────────────────────────────────────────────

/// Valkey TTL for the model list cache (1 hour).
const MODELS_CACHE_TTL: i64 = 3600;

fn models_cache_key(id: Uuid) -> String {
    format!("veronex:models:{id}")
}

/// Fetch the list of available models directly from the provider (bypasses cache).
///
/// * Ollama → `GET {url}/api/tags`
/// * Gemini → `GET https://generativelanguage.googleapis.com/v1beta/models?key={api_key}`
///   filtered to models that support `generateContent`.
async fn fetch_models_live(provider: &LlmProvider) -> Result<Vec<String>> {
    let client = reqwest::Client::new();

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
                .unwrap_or(&vec![])
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
        tracing::warn!("failed to cache model list: {e}");
    }
}

/// Read models from the Valkey cache. Returns `None` on miss or error.
async fn load_models_cache(pool: &fred::clients::Pool, key: &str) -> Option<Vec<String>> {
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
    /// FK → gpu_servers. Optional; Gemini providers leave this null.
    pub server_id: Option<Uuid>,
    /// veronex-agent URL (Phase 2, reserved). E.g. `"http://192.168.1.10:9091"`.
    pub agent_url: Option<String>,
    /// true = key is on a Google free-tier project.
    /// RPM/RPD limits are managed globally via `gemini_rate_limit_policies`.
    pub is_free_tier: Option<bool>,
}

/// Update request for `PATCH /v1/providers/{id}`.
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
    pub is_free_tier: Option<bool>,
    /// Enable or disable the provider for routing.
    pub is_active: Option<bool>,
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
    pub is_free_tier: bool,
    pub status: String,
    pub registered_at: DateTime<Utc>,
    /// Masked API key shown in the management UI (e.g. `AIza...x1y2`). Gemini only.
    pub api_key_masked: Option<String>,
}

impl From<LlmProvider> for BackendSummary {
    fn from(b: LlmProvider) -> Self {
        let backend_type = match b.provider_type {
            ProviderType::Ollama => "ollama",
            ProviderType::Gemini => "gemini",
        }
        .to_string();
        let status = match b.status {
            LlmProviderStatus::Online => "online",
            LlmProviderStatus::Offline => "offline",
            LlmProviderStatus::Degraded => "degraded",
        }
        .to_string();
        let api_key_masked = b.api_key_encrypted.as_ref().map(|k| {
            if k.len() <= 8 {
                "****".to_string()
            } else {
                format!("{}...{}", &k[..4], &k[k.len() - 4..])
            }
        });
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
            is_free_tier: b.is_free_tier,
            status,
            registered_at: b.registered_at,
            api_key_masked,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RegisterBackendResponse {
    pub id: Uuid,
    pub status: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn provider_registry(state: &AppState) -> &Arc<dyn LlmProviderRegistry> {
    &state.provider_registry
}

fn parse_provider_type(s: &str) -> Option<ProviderType> {
    match s.to_lowercase().as_str() {
        "ollama" => Some(ProviderType::Ollama),
        "gemini" => Some(ProviderType::Gemini),
        _ => None,
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /v1/providers` — register a new Ollama or Gemini provider.
///
/// Immediately runs a health check and sets the initial status.
pub async fn register_provider(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Json(req): Json<RegisterBackendRequest>,
) -> impl IntoResponse {
    let Some(provider_type) = parse_provider_type(&req.backend_type) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "backend_type must be 'ollama' or 'gemini'"})),
        )
            .into_response();
    };

    // Validate required fields per provider type.
    match provider_type {
        ProviderType::Ollama => {
            if req.url.as_deref().unwrap_or("").is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "url is required for ollama providers"})),
                )
                    .into_response();
            }
        }
        ProviderType::Gemini => {
            if req.api_key.as_deref().unwrap_or("").is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "api_key is required for gemini providers"})),
                )
                    .into_response();
            }
        }
    }

    let provider = LlmProvider {
        id: Uuid::now_v7(),
        name: req.name.clone(),
        provider_type: provider_type.clone(),
        url: req.url.unwrap_or_default(),
        api_key_encrypted: req.api_key,
        is_active: true,
        total_vram_mb: req.total_vram_mb.unwrap_or(0),
        gpu_index: req.gpu_index,
        server_id: req.server_id,
        agent_url: req.agent_url.filter(|s| !s.is_empty()),
        is_free_tier: req.is_free_tier.unwrap_or(false),
        status: LlmProviderStatus::Offline, // initial; overwritten by health check
        registered_at: Utc::now(),
    };

    // Health check before persisting.
    let client = reqwest::Client::new();
    let initial_status = check_provider(&client, &provider).await;
    let provider = LlmProvider {
        status: initial_status.clone(),
        ..provider
    };

    let registry = provider_registry(&state);
    if let Err(e) = registry.register(&provider).await {
        tracing::error!("failed to register provider: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    let status_str = match initial_status {
        LlmProviderStatus::Online => "online",
        LlmProviderStatus::Offline => "offline",
        LlmProviderStatus::Degraded => "degraded",
    };

    tracing::info!(
        id = %provider.id,
        name = %provider.name,
        provider_type = %req.backend_type,
        status = %status_str,
        "provider registered"
    );

    let resource_type = match provider.provider_type {
        ProviderType::Ollama => "ollama_provider",
        ProviderType::Gemini => "gemini_provider",
    };
    emit_audit(&state, &claims, "create", resource_type, &provider.id.to_string(), &provider.name,
        &format!("Provider '{}' registered (type: {}, initial_status: {})",
            provider.name, req.backend_type, status_str)).await;

    (
        StatusCode::CREATED,
        Json(RegisterBackendResponse {
            id: provider.id,
            status: status_str.to_string(),
        }),
    )
        .into_response()
}

/// `GET /v1/providers` — list all registered providers.
pub async fn list_providers(State(state): State<AppState>) -> impl IntoResponse {
    match provider_registry(&state).list_all().await {
        Ok(providers) => {
            let summaries: Vec<BackendSummary> = providers.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(summaries)).into_response()
        }
        Err(e) => {
            tracing::error!("failed to list providers: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `DELETE /v1/providers/{id}` — soft-delete (deactivate) a provider.
pub async fn delete_provider(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Fetch name before deactivating for the audit record.
    let name = provider_registry(&state).get(id).await.ok().flatten().map(|b| b.name).unwrap_or_default();

    match provider_registry(&state).deactivate(id).await {
        Ok(()) => {
            emit_audit(&state, &claims, "delete", "ollama_provider", &id.to_string(), &name,
                &format!("Provider '{}' ({}) deactivated (soft-deleted, no longer routed)", name, id)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            tracing::error!(%id, "failed to deactivate provider: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `POST /v1/providers/{id}/healthcheck` — manually trigger a health check.
pub async fn healthcheck_provider(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let registry = provider_registry(&state);

    let provider = match registry.get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "provider not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "failed to fetch provider: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    let client = reqwest::Client::new();
    let new_status = check_provider(&client, &provider).await;

    if let Err(e) = registry.update_status(id, new_status.clone()).await {
        tracing::warn!(%id, "failed to persist healthcheck result: {e}");
    }

    let status_str = match new_status {
        LlmProviderStatus::Online => "online",
        LlmProviderStatus::Offline => "offline",
        LlmProviderStatus::Degraded => "degraded",
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({"id": id, "status": status_str})),
    )
        .into_response()
}

/// `PATCH /v1/providers/{id}` — update mutable fields of a provider.
///
/// All fields are optional; only provided (non-null) fields are applied.
/// Passing `api_key: ""` leaves the existing key unchanged.
pub async fn update_provider(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateBackendRequest>,
) -> impl IntoResponse {
    let registry = provider_registry(&state);

    let mut provider = match registry.get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "provider not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "update_provider: db error: {e}");
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
    provider.name = req.name.trim().to_string();
    if let Some(url) = req.url {
        provider.url = url;
    }
    // Empty / absent api_key = keep existing stored value.
    if let Some(key) = req.api_key.filter(|s| !s.is_empty()) {
        provider.api_key_encrypted = Some(key);
    }
    provider.total_vram_mb = req.total_vram_mb.unwrap_or(provider.total_vram_mb);
    provider.gpu_index = req.gpu_index;   // null clears the field
    provider.server_id = req.server_id;  // null clears the field
    if let Some(v) = req.is_free_tier { provider.is_free_tier = v; }
    if let Some(v) = req.is_active { provider.is_active = v; }

    if let Err(e) = registry.update(&provider).await {
        tracing::error!(%id, "update_provider: failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
            .into_response();
    }

    let resource_type = match provider.provider_type {
        ProviderType::Ollama => "ollama_provider",
        ProviderType::Gemini => "gemini_provider",
    };
    emit_audit(&state, &claims, "update", resource_type, &id.to_string(), &provider.name,
        &format!("Provider '{}' ({}) configuration updated", provider.name, id)).await;
    tracing::info!(%id, "provider updated");
    (StatusCode::OK, Json(BackendSummary::from(provider))).into_response()
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_summary_from_llm_provider_ollama() {
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
            agent_url: None,
            is_free_tier: false,
            status: LlmProviderStatus::Online,
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
    fn backend_summary_from_llm_provider_gemini() {
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
            agent_url: None,
            is_free_tier: true,
            status: LlmProviderStatus::Offline,
            registered_at: Utc::now(),
        };
        let s = BackendSummary::from(b);
        assert_eq!(s.backend_type, "gemini");
        assert_eq!(s.status, "offline");
    }

    #[test]
    fn parse_provider_type_case_insensitive() {
        assert_eq!(parse_provider_type("Ollama"), Some(ProviderType::Ollama));
        assert_eq!(parse_provider_type("GEMINI"), Some(ProviderType::Gemini));
        assert_eq!(parse_provider_type("unknown"), None);
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

/// `GET /v1/providers/{id}/models` — list models available on a provider.
///
/// Returns the cached model list if available (TTL: 1 h).
/// On cache miss, fetches live from Ollama (`/api/tags`) or the Gemini models API,
/// stores the result in Valkey, and returns it.
pub async fn list_provider_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match provider_registry(&state).get(id).await {
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "provider not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "failed to fetch provider: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
        Ok(Some(_)) => {}
    };

    let cache_key = models_cache_key(id);

    // ── Cache hit ────────────────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool {
        if let Some(models) = load_models_cache(pool, &cache_key).await {
            return (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response();
        }
    }

    // ── Cache miss: return empty — use POST /v1/providers/{id}/models/sync to populate ──
    (StatusCode::OK, Json(serde_json::json!({"models": []}))).into_response()
}

/// `GET /v1/providers/{id}/key` — return the stored (plain-text PoC) API key for a Gemini provider.
///
/// Requires admin auth. Returns `{"key": "AIza..."}`.
pub async fn reveal_provider_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let provider = match provider_registry(&state).get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "provider not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "reveal_provider_key: db error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    match provider.api_key_encrypted {
        Some(key) => (StatusCode::OK, Json(serde_json::json!({"key": key}))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no api key stored for this provider"})),
        )
            .into_response(),
    }
}

/// `POST /v1/providers/{id}/models/sync` — force-refresh the model list from the provider.
///
/// For Ollama providers: ignores Valkey cache, fetches live, stores the fresh list.
/// For Gemini providers: returns 400 — use `POST /v1/gemini/models/sync` instead.
pub async fn sync_provider_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let provider = match provider_registry(&state).get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "provider not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "failed to fetch provider: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    // Gemini model sync is global — direct the caller to the correct endpoint.
    if matches!(provider.provider_type, ProviderType::Gemini) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Use POST /v1/gemini/models/sync to sync Gemini models globally"
            })),
        )
            .into_response();
    }

    match fetch_models_live(&provider).await {
        Ok(models) => {
            let cache_key = models_cache_key(id);
            if let Some(ref pool) = state.valkey_pool {
                store_models_cache(pool, &cache_key, &models).await;
            }
            // Also persist to the global ollama_models table.
            if let Err(e) = state.ollama_model_repo.sync_provider_models(id, &models).await {
                tracing::warn!(%id, "failed to persist ollama models to DB (non-fatal): {e}");
            }
            // Upsert model selections (is_enabled defaults to true for new rows).
            if let Err(e) = state.model_selection_repo.upsert_models(id, &models).await {
                tracing::warn!(%id, "failed to upsert model selections (non-fatal): {e}");
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

// ── Selected-model handlers ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SelectedModelDto {
    model_name: String,
    is_enabled: bool,
    synced_at: DateTime<Utc>,
}

/// `GET /v1/providers/{id}/selected-models` — list models with per-provider enabled state.
///
/// **Ollama**: merges per-provider `ollama_models` with `backend_selected_models`.
///   New models default to `is_enabled = true`.
/// **Gemini**: merges the global `gemini_models` pool with `backend_selected_models`.
///   New models default to `is_enabled = false`.
pub async fn list_selected_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Resolve the provider to branch by type.
    let provider = match state.provider_registry.get(id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "provider not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(%id, "list_selected_models: failed to fetch provider: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    // Per-provider selections (enabled/disabled overrides).
    let selections = match state.model_selection_repo.list(id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%id, "list_selected_models: failed to list selections: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
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
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "database error"})),
                    )
                        .into_response();
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
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "database error"})),
                    )
                        .into_response();
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

#[derive(Debug, Deserialize)]
pub struct SetModelEnabledRequest {
    pub is_enabled: bool,
}

/// `PATCH /v1/providers/{id}/selected-models/{model_name}` — toggle a model's enabled state.
pub async fn set_model_enabled(
    State(state): State<AppState>,
    Path((id, model_name)): Path<(Uuid, String)>,
    Json(req): Json<SetModelEnabledRequest>,
) -> impl IntoResponse {
    match state
        .model_selection_repo
        .set_enabled(id, &model_name, req.is_enabled)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(%id, %model_name, "set_model_enabled: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}
