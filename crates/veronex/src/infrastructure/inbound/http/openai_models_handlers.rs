//! GET /v1/models and GET /v1/models/{model_id} — OpenAI-compatible model listing.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::Serialize;
use tracing::instrument;

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

/// `GET /v1/models` — list all available models across Ollama + Gemini providers.
#[instrument(skip(state))]
pub async fn list_models(State(state): State<AppState>) -> Result<Response, AppError> {
    let now = Utc::now().timestamp();

    // Fetch from both repos in parallel.
    let (ollama_result, gemini_result) = tokio::join!(
        state.ollama_model_repo.list_all(),
        state.gemini_model_repo.list(),
    );

    let mut models: Vec<ModelObject> = Vec::new();

    if let Ok(ollama_models) = ollama_result {
        models.reserve(ollama_models.len());
        for name in ollama_models {
            models.push(ModelObject {
                id: name,
                object: "model",
                created: now,
                owned_by: "ollama".to_string(),
            });
        }
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

/// `GET /v1/models/{model_id}` — get a single model by ID.
#[instrument(skip(state), fields(model_id = %model_id))]
pub async fn get_model(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Response, AppError> {
    let now = Utc::now().timestamp();

    // Fetch from both repos in parallel to avoid sequential round-trips.
    let (ollama_result, gemini_result) = tokio::join!(
        state.ollama_model_repo.list_all(),
        state.gemini_model_repo.list(),
    );

    if let Ok(all) = ollama_result {
        if all.contains(&model_id) {
            return Ok(Json(ModelObject {
                id: model_id,
                object: "model",
                created: now,
                owned_by: "ollama".to_string(),
            }).into_response());
        }
    }

    if let Ok(gemini_models) = gemini_result {
        if gemini_models.iter().any(|m| m.model_name == model_id) {
            return Ok(Json(ModelObject {
                id: model_id,
                object: "model",
                created: now,
                owned_by: "google".to_string(),
            }).into_response());
        }
    }

    Err(AppError::NotFound(format!("The model '{model_id}' does not exist")))
}
