use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::enums::ProviderType;

use super::state::AppState;

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncJobResponse {
    pub job_id: Uuid,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct OllamaSyncJobDto {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub total_providers: i32,
    pub done_providers: i32,
    pub results: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct OllamaModelDto {
    pub model_name: String,
    pub provider_count: i64,
}

#[derive(Debug, Serialize)]
pub struct OllamaProviderDto {
    pub provider_id: Uuid,
    pub name: String,
    pub url: String,
    pub status: String,
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/ollama/models` — list all distinct Ollama model names with provider counts.
pub async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    match state.ollama_model_repo.list_with_counts().await {
        Ok(models) => {
            let dtos: Vec<OllamaModelDto> = models
                .into_iter()
                .map(|m| OllamaModelDto {
                    model_name: m.model_name,
                    provider_count: m.provider_count,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"models": dtos}))).into_response()
        }
        Err(e) => {
            tracing::error!("ollama list_models: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `GET /v1/ollama/models/:model_name/providers`
/// — list providers (id, name, url, status) that have the given model synced.
pub async fn list_model_providers(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> impl IntoResponse {
    match state
        .ollama_model_repo
        .providers_info_for_model(&model_name)
        .await
    {
        Ok(backends) => {
            let dtos: Vec<OllamaProviderDto> = backends
                .into_iter()
                .map(|b| OllamaProviderDto {
                    provider_id: b.provider_id,
                    name: b.name,
                    url: b.url,
                    status: b.status,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"backends": dtos}))).into_response()
        }
        Err(e) => {
            tracing::error!("ollama list_model_providers: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `GET /v1/ollama/providers/:provider_id/models`
/// — list model names synced for a specific Ollama provider.
pub async fn list_backend_models(
    State(state): State<AppState>,
    Path(provider_id): Path<Uuid>,
) -> impl IntoResponse {
    match state.ollama_model_repo.models_for_provider(provider_id).await {
        Ok(models) => {
            (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response()
        }
        Err(e) => {
            tracing::error!("ollama list_backend_models: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}

/// `POST /v1/ollama/models/sync` — trigger a global background sync of all Ollama providers.
///
/// Returns 202 immediately with the job ID. The sync runs in the background,
/// processing each provider sequentially without retrying on failure.
pub async fn sync_all_providers(State(state): State<AppState>) -> impl IntoResponse {
    // List all active Ollama providers.
    let backends = match state.provider_registry.list_all().await {
        Ok(all) => {
            let ollama: Vec<_> = all
                .into_iter()
                .filter(|b| b.is_active && b.provider_type == ProviderType::Ollama)
                .collect();
            ollama
        }
        Err(e) => {
            tracing::error!("sync_all_providers: failed to list providers: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    if backends.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no active Ollama providers registered"})),
        )
            .into_response();
    }

    let total = backends.len() as i32;
    let job_id = match state.ollama_sync_job_repo.create(total).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("sync_all_providers: failed to create sync job: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    // Clone Arcs for the background task.
    let ollama_model_repo = state.ollama_model_repo.clone();
    let ollama_sync_job_repo = state.ollama_sync_job_repo.clone();
    let model_selection_repo = state.model_selection_repo.clone();

    tokio::spawn(async move {
        let client = reqwest::Client::new();

        for provider in backends {
            let url = format!("{}/api/tags", provider.url.trim_end_matches('/'));

            let result = async {
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

                let models: Vec<String> = json["models"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect();

                ollama_model_repo
                    .sync_provider_models(provider.id, &models)
                    .await?;

                anyhow::Ok(models)
            }
            .await;

            let progress_entry = match result {
                Ok(models) => {
                    // Upsert model selections (is_enabled defaults to true for new rows).
                    if let Err(e) = model_selection_repo.upsert_models(provider.id, &models).await {
                        tracing::warn!(provider_id = %provider.id, "upsert model selections failed (non-fatal): {e}");
                    }
                    tracing::info!(
                        provider_id = %provider.id,
                        name = %provider.name,
                        count = models.len(),
                        "ollama provider synced"
                    );
                    serde_json::json!({
                        "provider_id": provider.id,
                        "name": provider.name,
                        "models": models,
                        "error": null
                    })
                }
                Err(e) => {
                    tracing::warn!(
                        provider_id = %provider.id,
                        name = %provider.name,
                        "ollama provider sync failed: {e}"
                    );
                    serde_json::json!({
                        "provider_id": provider.id,
                        "name": provider.name,
                        "models": [],
                        "error": e.to_string()
                    })
                }
            };

            if let Err(e) = ollama_sync_job_repo
                .update_progress(job_id, progress_entry)
                .await
            {
                tracing::error!(%job_id, "failed to update sync job progress: {e}");
            }
        }

        if let Err(e) = ollama_sync_job_repo.complete(job_id).await {
            tracing::error!(%job_id, "failed to mark sync job completed: {e}");
        }

        tracing::info!(%job_id, "ollama global sync completed");
    });

    (
        StatusCode::ACCEPTED,
        Json(SyncJobResponse {
            job_id,
            status: "running".to_string(),
        }),
    )
        .into_response()
}

/// `GET /v1/ollama/sync/status` — return the latest sync job status.
pub async fn get_sync_status(State(state): State<AppState>) -> impl IntoResponse {
    match state.ollama_sync_job_repo.get_latest().await {
        Ok(Some(job)) => {
            let dto = OllamaSyncJobDto {
                id: job.id,
                started_at: job.started_at,
                completed_at: job.completed_at,
                status: job.status,
                total_providers: job.total_providers,
                done_providers: job.done_providers,
                results: job.results,
            };
            (StatusCode::OK, Json(dto)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no sync job found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("get_sync_status: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response()
        }
    }
}
