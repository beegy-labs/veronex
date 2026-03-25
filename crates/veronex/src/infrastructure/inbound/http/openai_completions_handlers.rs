//! POST /v1/completions — OpenAI-compatible legacy text completions endpoint.
//! Maps single-prompt requests to the Veronex inference queue.

use axum::extract::State;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;
use tracing::instrument;

use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::domain::enums::{ApiFormat, FinishReason, ProviderType};
use super::constants::{ERR_MODEL_INVALID, ERR_PROMPT_TOO_LARGE};
use super::error::AppError;
use super::handlers::sanitize_sse_error;
use super::inference_helpers::{build_sse_response, extract_conversation_id, validate_model_name, validate_content_length};
use super::openai_sse_types::SYSTEM_FINGERPRINT;
use super::middleware::infer_auth::InferCaller;
use super::state::AppState;

#[derive(Deserialize)]
pub struct TextCompletionRequest {
    pub model: String,
    pub prompt: serde_json::Value, // string or array
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub stop: Option<serde_json::Value>,
    #[serde(default)]
    pub seed: Option<u32>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(default)]
    pub frequency_penalty: Option<f64>,
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    #[serde(default)]
    pub provider_type: Option<String>,
}

impl TextCompletionRequest {
    fn prompt_str(&self) -> String {
        match &self.prompt {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}

/// `POST /v1/completions` — legacy text completion endpoint.
#[instrument(skip(state, req, headers), fields(model = %req.model))]
pub async fn text_completions(
    State(state): State<AppState>,
    axum::extract::Extension(caller): axum::extract::Extension<InferCaller>,
    headers: axum::http::HeaderMap,
    Json(req): Json<TextCompletionRequest>,
) -> Result<Response, AppError> {
    validate_model_name(&req.model)
        .map_err(|_| AppError::BadRequest(ERR_MODEL_INVALID.into()))?;

    let prompt = req.prompt_str();
    validate_content_length(prompt.len())
        .map_err(|_| AppError::BadRequest(ERR_PROMPT_TOO_LARGE.into()))?;

    if prompt.is_empty() {
        return Err(AppError::BadRequest("prompt cannot be empty".into()));
    }

    let model = req.model.clone();
    let stream = req.stream.unwrap_or(false);
    let conversation_id = extract_conversation_id(&headers);

    let job_id = state.use_case.submit(SubmitJobRequest {
        prompt,
        model_name: model.clone(),
        provider_type: ProviderType::Ollama,
        gemini_tier: None,
        api_key_id: caller.api_key_id(),
        account_id: caller.account_id(),
        source: caller.source(),
        api_format: ApiFormat::OpenaiCompat,
        messages: None,
        tools: None,
        request_path: Some("/v1/completions".to_string()),
        conversation_id,
        key_tier: caller.key_tier(),
        images: None,
        stop: req.stop,
        seed: req.seed,
        response_format: None,
        frequency_penalty: req.frequency_penalty,
        presence_penalty: req.presence_penalty,
        mcp_loop_id: None,
    }).await.map_err(|e| {
        tracing::error!("text_completions: submit failed: {e}");
        AppError::Internal(anyhow::anyhow!("failed to submit inference job"))
    })?;

    let chunk_id = format!("cmpl-{}", job_id.0);
    let created = chrono::Utc::now().timestamp();

    if !stream {
        // Collect all tokens
        let mut token_stream = state.use_case.stream(&job_id);
        let mut text = String::new();
        let mut prompt_tokens: u32 = 0;
        let mut completion_tokens: u32 = 0;
        let mut finish_reason = FinishReason::Stop.as_str().to_string();
        while let Some(result) = token_stream.next().await {
            match result {
                Ok(t) if t.is_final => {
                    prompt_tokens = t.prompt_tokens.unwrap_or(0);
                    completion_tokens = t.completion_tokens.unwrap_or(completion_tokens);
                    finish_reason = t.finish_reason.unwrap_or_else(|| FinishReason::Stop.as_str().to_string());
                    break;
                }
                Ok(t) => { if !t.value.is_empty() { text.push_str(&t.value); } }
                Err(e) => {
                    return Err(AppError::Internal(anyhow::anyhow!("{}", sanitize_sse_error(&e))));
                }
            }
        }
        return Ok(Json(serde_json::json!({
            "id": chunk_id,
            "object": "text_completion",
            "created": created,
            "model": model,
            "system_fingerprint": SYSTEM_FINGERPRINT,
            "choices": [{
                "text": text,
                "index": 0,
                "logprobs": null,
                "finish_reason": finish_reason,
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens,
            }
        })).into_response());
    }

    Ok(build_sse_response(&state, job_id, true, move |result| {
        match result {
            Ok(token) if token.is_final => {
                let reason = token.finish_reason.as_deref().unwrap_or(FinishReason::Stop.as_str());
                let chunk = serde_json::json!({
                    "id": chunk_id,
                    "object": "text_completion",
                    "created": created,
                    "model": model,
                    "choices": [{"text": "", "index": 0, "logprobs": null, "finish_reason": reason}]
                });
                vec![Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())]
            }
            Ok(token) if token.value.is_empty() => vec![],
            Ok(token) => {
                let chunk = serde_json::json!({
                    "id": chunk_id,
                    "object": "text_completion",
                    "created": created,
                    "model": model,
                    "choices": [{"text": token.value, "index": 0, "logprobs": null, "finish_reason": null}]
                });
                vec![Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())]
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": sanitize_sse_error(&e)}});
                vec![Event::default().data(serde_json::to_string(&err).unwrap_or_default())]
            }
        }
    }))
}
