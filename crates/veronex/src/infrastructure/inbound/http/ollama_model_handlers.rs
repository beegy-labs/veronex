use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::enums::ProviderType;

use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSuper;

use super::constants::ERR_DATABASE;
use super::error::error_json;
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
            error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response()
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
        Ok(providers) => {
            let dtos: Vec<OllamaProviderDto> = providers
                .into_iter()
                .map(|p| OllamaProviderDto {
                    provider_id: p.provider_id,
                    name: p.name,
                    url: p.url,
                    status: p.status,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"providers": dtos}))).into_response()
        }
        Err(e) => {
            tracing::error!("ollama list_model_providers: {e}");
            error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response()
        }
    }
}

/// `GET /v1/ollama/providers/:provider_id/models`
/// — list model names synced for a specific Ollama provider.
pub async fn list_provider_models(
    State(state): State<AppState>,
    Path(provider_id): Path<Uuid>,
) -> impl IntoResponse {
    match state.ollama_model_repo.models_for_provider(provider_id).await {
        Ok(models) => {
            (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response()
        }
        Err(e) => {
            tracing::error!("ollama list_provider_models: {e}");
            error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response()
        }
    }
}

/// `POST /v1/ollama/models/sync` — trigger a global background sync of all Ollama providers.
///
/// Returns 202 immediately with the job ID. The sync runs in the background,
/// processing each provider sequentially without retrying on failure.
pub async fn sync_all_providers(
    RequireSuper(_claims): RequireSuper,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // List all active Ollama providers.
    let providers = match state.provider_registry.list_all().await {
        Ok(all) => {
            let ollama: Vec<_> = all
                .into_iter()
                .filter(|p| p.is_active && p.provider_type == ProviderType::Ollama)
                .collect();
            ollama
        }
        Err(e) => {
            tracing::error!("sync_all_providers: failed to list providers: {e}");
            return error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response();
        }
    };

    if providers.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no active Ollama providers registered"})),
        )
            .into_response();
    }

    let total = providers.len() as i32;
    let job_id = match state.ollama_sync_job_repo.create(total).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("sync_all_providers: failed to create sync job: {e}");
            return error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response();
        }
    };

    // Clone Arcs for the background task.
    let http_client = state.http_client.clone();
    let ollama_model_repo = state.ollama_model_repo.clone();
    let ollama_sync_job_repo = state.ollama_sync_job_repo.clone();
    let model_selection_repo = state.model_selection_repo.clone();

    tokio::spawn(async move {
        let client = http_client;

        for provider in providers {
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
                    .map_or(&[] as &[_], |v| v)
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
pub async fn get_sync_status(
    RequireSuper(_claims): RequireSuper,
    State(state): State<AppState>,
) -> impl IntoResponse {
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
            error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response()
        }
    }
}

// ── Pull Drain (SDD §5) ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PullModelRequest {
    pub model: String,
    pub provider_id: Uuid,
}

/// `POST /v1/ollama/models/pull` — Pull drain sequence (SDD §5).
///
/// Sets is_pulling=true, waits for active_requests==0 (60s drain),
/// executes Ollama pull, then resets AIMD epoch. Requires admin auth.
/// Returns 202 Accepted immediately; pull runs in background.
pub async fn pull_model(
    _: RequireSuper,
    State(state): State<AppState>,
    Json(req): Json<PullModelRequest>,
) -> impl IntoResponse {
    let provider_id = req.provider_id;
    let model = req.model.clone();

    // Verify provider exists and is Ollama
    let provider = match state.provider_registry.get(provider_id).await {
        Ok(Some(p)) if p.provider_type == ProviderType::Ollama => p,
        Ok(Some(_)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "provider is not Ollama type"})),
            ).into_response();
        }
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "provider not found"}))).into_response();
        }
        Err(e) => {
            tracing::error!(%provider_id, "pull_model: registry lookup failed: {e}");
            return error_json(StatusCode::INTERNAL_SERVER_ERROR, ERR_DATABASE).into_response();
        }
    };

    // Set is_pulling=true to block dispatch routing immediately
    state.vram_pool.set_pulling(provider_id, &model, true);
    tracing::info!(%provider_id, %model, "pull drain initiated — dispatch blocked");

    // Spawn background: drain → pull → AIMD reset
    let vram_c = state.vram_pool.clone();
    let client = state.http_client.clone();
    let base_url = provider.url.clone();
    let model_c = model.clone();

    tokio::spawn(async move {
        crate::infrastructure::outbound::ollama::preloader::pull_and_reset(
            &client, &base_url, &model_c, provider_id, &vram_c,
        ).await;
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "message": "pull drain started",
            "provider_id": provider_id,
            "model": model,
        })),
    ).into_response()
}
