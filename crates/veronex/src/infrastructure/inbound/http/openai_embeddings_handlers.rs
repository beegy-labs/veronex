//! POST /v1/embeddings — OpenAI-compatible embeddings endpoint.
//! Proxies to the best available Ollama provider's /api/embed.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::domain::enums::ProviderType;
use super::constants::ERR_MODEL_INVALID;
use super::error::AppError;
use super::inference_helpers::{validate_content_length, validate_model_name};
use super::middleware::infer_auth::InferCaller;
use super::provider_validation::validate_provider_url;
use super::state::AppState;

#[derive(Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
    #[serde(default)]
    pub encoding_format: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

impl EmbeddingInput {
    fn into_strings(self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s],
            Self::Multiple(v) => v,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingObject {
    object: &'static str,
    embedding: Vec<f64>,
    index: usize,
}

#[derive(Serialize)]
struct EmbeddingResponse {
    object: &'static str,
    data: Vec<EmbeddingObject>,
    model: String,
    usage: EmbeddingUsage,
}

#[derive(Serialize)]
struct EmbeddingUsage {
    prompt_tokens: u32,
    total_tokens: u32,
}

/// `POST /v1/embeddings` — generate embeddings in OpenAI-compatible format.
/// Proxies to the best available Ollama provider's `/api/embed`.
#[instrument(skip(state, req), fields(model = %req.model))]
pub async fn create_embeddings(
    State(state): State<AppState>,
    axum::extract::Extension(_caller): axum::extract::Extension<InferCaller>,
    Json(req): Json<EmbeddingRequest>,
) -> Result<Response, AppError> {
    validate_model_name(&req.model)
        .map_err(|_| AppError::BadRequest(ERR_MODEL_INVALID.into()))?;

    // Validate total input content length against MAX_PROMPT_BYTES.
    let inputs = req.input.into_strings();
    let total_input_bytes: usize = inputs.iter().map(|s| s.len()).sum();
    validate_content_length(total_input_bytes)
        .map_err(|e| AppError::BadRequest(e.into()))?;

    // Pick an Ollama provider.
    // Note: embeddings bypass the VRAM-aware ProviderDispatchPort because /api/embed does
    // not consume VRAM in the same way as inference. We pick the first active Ollama provider.
    // Future: consider routing through pick_and_build once InferenceProviderPort exposes an embed method.
    let providers = state.provider_registry.list_active().await
        .map_err(|e| {
            tracing::error!("embeddings: failed to list providers: {e}");
            AppError::ServiceUnavailable("no providers available".into())
        })?;

    let provider = providers.into_iter()
        .find(|p| p.provider_type == ProviderType::Ollama)
        .ok_or_else(|| AppError::ServiceUnavailable("no Ollama provider available for embeddings".into()))?;

    // SSRF prevention: validate the provider URL before making an outbound request.
    validate_provider_url(&provider.url)?;

    let model = req.model.clone();

    // Call Ollama /api/embed
    let ollama_url = format!("{}/api/embed", provider.url);
    let body = serde_json::json!({
        "model": model,
        "input": inputs,
    });

    let resp = state.http_client.post(&ollama_url).json(&body).send().await
        .map_err(|e| {
            tracing::error!("embeddings: Ollama request failed: {e}");
            AppError::BadGateway("provider request failed".into())
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let msg = resp.text().await.unwrap_or_default();
        tracing::error!("embeddings: Ollama returned {status}: {msg}");
        return Err(AppError::BadGateway(format!("provider returned {status}")));
    }

    let ollama_resp: serde_json::Value = resp.json().await
        .map_err(|e| AppError::BadGateway(format!("failed to parse provider response: {e}")))?;

    // Ollama /api/embed returns { "embeddings": [[...], [...]], "prompt_eval_count": N }
    let embeddings = ollama_resp.get("embeddings")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();

    let prompt_tokens = ollama_resp.get("prompt_eval_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let data: Vec<EmbeddingObject> = embeddings.into_iter().enumerate().map(|(i, emb)| {
        let embedding: Vec<f64> = emb.as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
            .unwrap_or_default();
        EmbeddingObject { object: "embedding", embedding, index: i }
    }).collect();

    Ok(Json(EmbeddingResponse {
        object: "list",
        data,
        model: req.model,
        usage: EmbeddingUsage { prompt_tokens, total_tokens: prompt_tokens },
    }).into_response())
}
