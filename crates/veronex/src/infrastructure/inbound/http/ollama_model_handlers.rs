use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::domain::enums::ProviderType;
use crate::domain::value_objects::{JobId, ProviderId};
use crate::infrastructure::inbound::http::inference_helpers::is_vision_model;

use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;

use super::audit_helpers::emit_audit;
use super::constants::ERR_DATABASE;
use super::error::error_json;
use super::handlers::ListPageParams;
use super::state::AppState;

// ── DTOs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncJobResponse {
    pub job_id: JobId,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct OllamaSyncJobDto {
    pub id: JobId,
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
    pub is_vision: bool,
    /// Maximum context window across all providers (0 = not yet profiled).
    pub max_ctx: u32,
    /// False if the model is disabled on all providers carrying it.
    pub is_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct OllamaProviderDto {
    pub provider_id: ProviderId,
    pub name: String,
    pub url: String,
    pub status: String,
    pub is_enabled: bool,
}

const DEFAULT_MODEL_LIMIT: i64 = 20;
const MAX_MODEL_LIMIT: i64 = 200;
const DEFAULT_PROVIDER_LIMIT: i64 = 10;
const MAX_PROVIDER_LIMIT: i64 = 100;

// ── Handlers ───────────────────────────────────────────────────────────────────

/// `GET /v1/ollama/models?search=&page=1&limit=20`
pub async fn list_models(
    State(state): State<AppState>,
    Query(params): Query<ListPageParams>,
) -> Result<Json<serde_json::Value>, super::error::AppError> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(DEFAULT_MODEL_LIMIT).clamp(1, MAX_MODEL_LIMIT);
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let offset = (page - 1) * limit;

    let page_result = state.ollama_model_repo.list_with_counts_page(&search, limit, offset).await
        .map_err(|e| { tracing::error!("ollama list_models: {e}"); super::error::AppError::Internal(e) })?;
    let dtos: Vec<OllamaModelDto> = page_result.items
        .into_iter()
        .map(|m| OllamaModelDto { is_vision: is_vision_model(&m.model_name), model_name: m.model_name, provider_count: m.provider_count, max_ctx: m.max_ctx.max(0) as u32, is_enabled: m.is_enabled })
        .collect();
    Ok(Json(serde_json::json!({
        "models": dtos,
        "total": page_result.total,
        "page": page,
        "limit": limit,
    })))
}

/// `GET /v1/ollama/models/:model_name/providers?search=&page=1&limit=10`
pub async fn list_model_providers(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
    Query(params): Query<ListPageParams>,
) -> Result<Json<serde_json::Value>, super::error::AppError> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(DEFAULT_PROVIDER_LIMIT).clamp(1, MAX_PROVIDER_LIMIT);
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let offset = (page - 1) * limit;

    let page_result = state.ollama_model_repo.providers_info_for_model_page(&model_name, &search, limit, offset).await
        .map_err(|e| { tracing::error!("ollama list_model_providers: {e}"); super::error::AppError::Internal(e) })?;
    let dtos: Vec<OllamaProviderDto> = page_result.items
        .into_iter()
        .map(|p| OllamaProviderDto {
            provider_id: ProviderId::from_uuid(p.provider_id),
            name: p.name,
            url: p.url,
            status: p.status,
            is_enabled: p.is_enabled,
        })
        .collect();
    Ok(Json(serde_json::json!({
        "providers": dtos,
        "total": page_result.total,
        "page": page,
        "limit": limit,
    })))
}

/// `POST /v1/ollama/models/sync` — trigger a global background sync of all Ollama providers.
///
/// Returns 202 immediately with the job ID. The sync runs in the background,
/// processing each provider sequentially without retrying on failure.
pub async fn sync_all_providers(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // List all active Ollama providers.
    let providers = match state.provider_registry.list_all().await {
        Ok(all) => {
            let ollama: Vec<_> = all
                .into_iter()
                .filter(|p| p.provider_type == ProviderType::Ollama)
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
                        "error": "sync failed"
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

    emit_audit(&state, &claims, "trigger", "ollama_sync",
        &job_id.to_string(), "global",
        &format!("Triggered Ollama model sync for {} providers", total)).await;

    (
        StatusCode::ACCEPTED,
        Json(SyncJobResponse {
            job_id: JobId::from_uuid(job_id),
            status: "running".to_string(),
        }),
    )
        .into_response()
}

/// `GET /v1/ollama/sync/status` — return the latest sync job status.
pub async fn get_sync_status(
    RequireProviderManage(_claims): RequireProviderManage,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.ollama_sync_job_repo.get_latest().await {
        Ok(Some(job)) => {
            let dto = OllamaSyncJobDto {
                id: JobId::from_uuid(job.id),
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
    pub provider_id: ProviderId,
}

/// `POST /v1/ollama/models/pull` — Pull drain sequence (SDD §5).
///
/// Sets is_pulling=true, waits for active_requests==0 (60s drain),
/// executes Ollama pull, then resets AIMD epoch. Requires admin auth.
/// Returns 202 Accepted immediately; pull runs in background.
pub async fn pull_model(
    RequireProviderManage(claims): RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<PullModelRequest>,
) -> impl IntoResponse {
    let provider_id = req.provider_id.0;
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

    emit_audit(&state, &claims, "trigger", "ollama_pull",
        &provider_id.to_string(), &model,
        &format!("Triggered pull drain for model '{}' on provider {}", model, provider_id)).await;

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "message": "pull drain started",
            "provider_id": provider_id,
            "model": model,
        })),
    ).into_response()
}
