//! GET /v1/models and GET /v1/models/{model_id} — OpenAI-compatible model listing.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::Serialize;
use tracing::instrument;

use crate::application::ports::outbound::modelfile_registry::ListFilter;

use super::constants::PROVIDER_LLAMA_SERVER;
use super::error::AppError;
use super::state::AppState;

#[derive(Serialize)]
struct ModelObject {
    id: String,
    object: &'static str,
    created: i64,
    owned_by: String,
}

#[derive(Serialize)]
struct ModelList {
    object: &'static str,
    data: Vec<ModelObject>,
}

/// `GET /v1/models` — every Modelfile registered in Veronex (Phase 2)
/// plus every Gemini model surfaced by the Gemini repo. The Modelfile
/// registry is the SSOT for self-hosted llama-server models.
#[instrument(skip(state))]
pub async fn list_models(State(state): State<AppState>) -> Result<Response, AppError> {
    let now = Utc::now().timestamp();

    let modelfile_fut = async {
        match state.modelfile_registry.as_ref() {
            Some(repo) => repo.list(&ListFilter::default()).await.unwrap_or_default(),
            None => Vec::new(),
        }
    };
    let gemini_fut = state.gemini_model_repo.list();
    let (modelfiles, gemini_result) = tokio::join!(modelfile_fut, gemini_fut);

    let mut models: Vec<ModelObject> = Vec::with_capacity(modelfiles.len());
    for m in modelfiles {
        models.push(ModelObject {
            id: m.model_id,
            object: "model",
            created: now,
            owned_by: PROVIDER_LLAMA_SERVER.to_string(),
        });
    }
    if let Ok(gemini_models) = gemini_result {
        models.reserve(gemini_models.len());
        for m in gemini_models {
            models.push(ModelObject {
                id: m.model_name,
                object: "model",
                created: now,
                owned_by: "google".to_string(),
            });
        }
    }

    Ok(Json(ModelList { object: "list", data: models }).into_response())
}

/// `GET /v1/models/{model_id}` — single-model lookup. Tries Modelfile
/// registry first (canonical "family:quantization" identifier), then
/// the Gemini list.
#[instrument(skip(state), fields(model_id = %model_id))]
pub async fn get_model(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Response, AppError> {
    let now = Utc::now().timestamp();

    if let Some(repo) = state.modelfile_registry.as_ref()
        && let Ok(Some(m)) = repo.get(&model_id).await
    {
        return Ok(Json(ModelObject {
            id: m.model_id,
            object: "model",
            created: now,
            owned_by: PROVIDER_LLAMA_SERVER.to_string(),
        })
        .into_response());
    }

    if let Ok(gemini_models) = state.gemini_model_repo.list().await
        && gemini_models.iter().any(|m| m.model_name == model_id)
    {
        return Ok(Json(ModelObject {
            id: model_id,
            object: "model",
            created: now,
            owned_by: "google".to_string(),
        })
        .into_response());
    }

    Err(AppError::NotFound(format!("The model '{model_id}' does not exist")))
}
