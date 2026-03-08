/// Ollama API-compatible gateway endpoints.
///
/// Exposes all standard Ollama API endpoints at their native paths (`/api/*`).
///
/// Inference endpoints (`/api/generate`, `/api/chat`) are routed through the
/// Veronex queue for VRAM-aware dispatch and thermal throttling.
///
/// Management endpoints (`/api/tags`, `/api/show`, `/api/ps`, etc.) proxy
/// directly to the first active Ollama provider (no queue needed).
///
/// Configure any Ollama client:
/// ```text
/// OLLAMA_HOST=http://localhost:3001
/// ```
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{Response as HttpResponse, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt as _;
use serde::Deserialize;

use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{ApiFormat, ProviderType, JobSource};
use super::cancel_guard::CancelOnDrop;
use super::constants::{ERR_MODEL_INVALID, ERR_PROMPT_TOO_LARGE};
use super::handlers::sanitize_sse_error;
use super::inference_helpers::{validate_model_name, validate_content_length, extract_last_user_prompt, extract_conversation_id};
use super::state::AppState;

// ── Inference request body types ────────────────────────────────────────────────

/// Fields deserialized for Ollama API compatibility but not all are forwarded.
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct OllamaGenerateBody {
    model: String,
    prompt: String,
    /// Text to append after the response (FIM / fill-in-the-middle).
    #[serde(default)]
    suffix: Option<String>,
    /// System prompt override.
    #[serde(default)]
    system: Option<String>,
    /// Base64-encoded images for multimodal requests.
    #[serde(default)]
    images: Option<Vec<String>>,
    /// Structured output format ("json" or JSON schema).
    #[serde(default)]
    format: Option<serde_json::Value>,
    /// Runtime generation options (temperature, num_ctx, top_p, …).
    #[serde(default)]
    options: Option<serde_json::Value>,
    /// Disable prompt templating (raw passthrough).
    #[serde(default)]
    raw: Option<bool>,
    /// How long to keep the model loaded after this request.
    #[serde(default)]
    keep_alive: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct OllamaChatBody {
    model: String,
    messages: Vec<serde_json::Value>,
    /// Tool/function definitions forwarded to the model.
    #[serde(default)]
    tools: Option<Vec<serde_json::Value>>,
    /// Structured output format.
    #[serde(default)]
    format: Option<serde_json::Value>,
    /// Runtime generation options.
    #[serde(default)]
    options: Option<serde_json::Value>,
    /// How long to keep the model loaded.
    #[serde(default)]
    keep_alive: Option<serde_json::Value>,
}

// ── Model listing (Veronex-owned) ───────────────────────────────────────────────

/// `GET /api/tags` — list all Veronex-synchronized Ollama models.
///
/// Returns Ollama-format response using models stored in the Veronex DB
/// (populated via the periodic sync job), not from a live Ollama query.
pub async fn list_local_models(State(state): State<AppState>) -> Response {
    let model_names = match state.ollama_model_repo.list_all().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("ollama_compat /api/tags: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to list models"})),
            )
                .into_response();
        }
    };

    let models: Vec<serde_json::Value> = model_names
        .into_iter()
        .map(|name| {
            serde_json::json!({
                "name":        name,
                "model":       name,
                "modified_at": chrono::Utc::now().to_rfc3339(),
                "size":        0,
                "digest":      "",
                "details": {
                    "parent_model":       "",
                    "format":             "gguf",
                    "family":             "",
                    "families":           [],
                    "parameter_size":     "",
                    "quantization_level": "",
                }
            })
        })
        .collect();

    Json(serde_json::json!({ "models": models })).into_response()
}

// ── Inference endpoints — queue-routed ──────────────────────────────────────────

/// `POST /api/generate` — text generation via Veronex queue (VRAM-aware dispatch).
///
/// Accepts Ollama's `/api/generate` request body and streams the response
/// as Ollama NDJSON (`application/x-ndjson`).
pub async fn generate(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    headers: axum::http::HeaderMap,
    Json(req): Json<OllamaGenerateBody>,
) -> Response {
    let conversation_id = extract_conversation_id(&headers);
    if validate_model_name(&req.model).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": ERR_MODEL_INVALID})),
        )
            .into_response();
    }
    if validate_content_length(req.prompt.len()).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": ERR_PROMPT_TOO_LARGE})),
        )
            .into_response();
    }
    if req.prompt.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "prompt is required"})),
        )
            .into_response();
    }

    let model = req.model.clone();

    let job_id = match state
        .use_case
        .submit(SubmitJobRequest {
            prompt: req.prompt,
            model_name: model.clone(),
            provider_type: ProviderType::Ollama,
            gemini_tier: None,
            api_key_id: Some(api_key.id),
            account_id: None,
            source: JobSource::Api,
            api_format: ApiFormat::OllamaNative,
            messages: None,
            tools: None,
            request_path: Some("/api/generate".to_string()),
            conversation_id,
            key_tier: Some(api_key.tier),
        })
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("ollama_compat generate: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to submit inference job"})),
            )
                .into_response();
        }
    };

    let token_stream = state.use_case.stream(&job_id);
    let model_clone = model.clone();

    let ndjson = token_stream.map(move |result| {
        let model = model_clone.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let line = match result {
            Ok(token) if token.is_final => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "response": "",
                "done": true,
                "done_reason": "stop",
                "total_duration": 0,
                "prompt_eval_count": token.prompt_tokens.unwrap_or(0),
                "eval_count": token.completion_tokens.unwrap_or(0),
            }),
            Ok(token) => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "response": token.value,
                "done": false,
            }),
            Err(e) => serde_json::json!({
                "error": sanitize_sse_error(&e),
                "done": true,
            }),
        };
        Ok::<_, std::convert::Infallible>(Bytes::from(format!("{}\n", line)))
    });

    let guarded = CancelOnDrop::new(ndjson, job_id, state.use_case.clone());
    HttpResponse::builder()
        .status(200)
        .header("Content-Type", "application/x-ndjson")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(guarded))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `POST /api/chat` — chat completion via Veronex queue (VRAM-aware dispatch).
///
/// Accepts Ollama's `/api/chat` request body and streams the response
/// as Ollama NDJSON (`application/x-ndjson`).
pub async fn chat(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    headers: axum::http::HeaderMap,
    Json(req): Json<OllamaChatBody>,
) -> Response {
    let conversation_id = extract_conversation_id(&headers);
    if validate_model_name(&req.model).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": ERR_MODEL_INVALID})),
        )
            .into_response();
    }
    // Validate total message content length.
    let total_content_len: usize = req.messages.iter()
        .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
        .map(|s| s.len())
        .sum();
    if validate_content_length(total_content_len).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": ERR_PROMPT_TOO_LARGE})),
        )
            .into_response();
    }
    // Extract last user message as display prompt (required by InferenceJob).
    // For native Ollama API, a user message is required.
    let has_user_msg = req.messages.iter().any(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));
    if !has_user_msg {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no user message found in messages array"})),
        )
            .into_response();
    }
    let prompt = extract_last_user_prompt(&req.messages).to_string();

    let model = req.model.clone();
    let messages = serde_json::Value::Array(req.messages);
    let tools = req.tools.map(serde_json::Value::Array);

    let job_id = match state
        .use_case
        .submit(SubmitJobRequest {
            prompt,
            model_name: model.clone(),
            provider_type: ProviderType::Ollama,
            gemini_tier: None,
            api_key_id: Some(api_key.id),
            account_id: None,
            source: JobSource::Api,
            api_format: ApiFormat::OllamaNative,
            messages: Some(messages),
            tools,
            request_path: Some("/api/chat".to_string()),
            conversation_id,
            key_tier: Some(api_key.tier),
        })
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("ollama_compat chat: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to submit inference job"})),
            )
                .into_response();
        }
    };

    let token_stream = state.use_case.stream(&job_id);
    let model_clone = model.clone();

    let ndjson = token_stream.map(move |result| {
        let model = model_clone.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let line = match result {
            Ok(token) if token.tool_calls.is_some() => {
                // Model returned tool calls — emit in Ollama NDJSON format.
                // The client (Qwen Code) expects message.tool_calls, not content.
                serde_json::json!({
                    "model": model,
                    "created_at": created_at,
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": token.tool_calls,
                    },
                    "done": false,
                })
            }
            Ok(token) if token.is_final => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "message": {"role": "assistant", "content": ""},
                "done": true,
                "done_reason": "stop",
                "total_duration": 0,
                "prompt_eval_count": token.prompt_tokens.unwrap_or(0),
                "eval_count": token.completion_tokens.unwrap_or(0),
            }),
            Ok(token) if token.value.is_empty() => return Ok(Bytes::new()),
            Ok(token) => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "message": {"role": "assistant", "content": token.value},
                "done": false,
            }),
            Err(e) => serde_json::json!({
                "error": sanitize_sse_error(&e),
                "done": true,
            }),
        };
        Ok::<_, std::convert::Infallible>(Bytes::from(format!("{}\n", line)))
    });

    let guarded = CancelOnDrop::new(ndjson, job_id, state.use_case.clone());
    HttpResponse::builder()
        .status(200)
        .header("Content-Type", "application/x-ndjson")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(guarded))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ── Management proxy endpoints ────────────────────────────────────────────────
//
// These endpoints do not perform inference and are not subject to VRAM/thermal
// constraints. They proxy directly to the first active Ollama provider.

/// Forward a request to the first active Ollama provider and stream the response back.
async fn proxy(state: &AppState, path: &str, req: Request) -> Response {
    let provider = match pick_ollama(state).await {
        Ok(b) => b,
        Err(r) => return r,
    };

    let method = match reqwest::Method::from_bytes(req.method().as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => reqwest::Method::POST,
    };

    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("ollama_compat proxy: failed to read body: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "failed to read request body"})),
            )
                .into_response();
        }
    };

    let url = format!("{}{}", provider.url, path);
    let client = state.http_client.clone();
    let mut builder = client.request(method, &url).header("Content-Type", &content_type);
    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes.to_vec());
    }

    let response = match builder.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("ollama_compat proxy to {url}: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "provider communication error"})),
            )
                .into_response();
        }
    };

    let status = response.status().as_u16();
    let resp_content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    let stream = response.bytes_stream();
    HttpResponse::builder()
        .status(status)
        .header("Content-Type", resp_content_type)
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `POST /api/show` — show model metadata.
pub async fn show(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/show", req).await
}

/// `POST /api/embed` — generate embeddings (Ollama ≥ 0.1.26).
pub async fn embed(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/embed", req).await
}

/// `POST /api/embeddings` — generate embeddings (legacy endpoint).
pub async fn embeddings(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/embeddings", req).await
}

/// `GET /api/ps` — list running models on the provider.
pub async fn ps(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/ps", req).await
}

/// `GET /api/version` — Ollama server version.
pub async fn version(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/version", req).await
}

/// `POST /api/pull` — pull a model (streams progress JSON).
pub async fn pull(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/pull", req).await
}

/// `POST /api/push` — push a model to a registry.
pub async fn push(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/push", req).await
}

/// `DELETE /api/delete` — delete a local model.
pub async fn delete(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/delete", req).await
}

/// `POST /api/copy` — copy a model.
pub async fn copy(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/copy", req).await
}

/// `POST /api/create` — create a model from a Modelfile.
pub async fn create(State(state): State<AppState>, req: Request) -> Response {
    proxy(&state, "/api/create", req).await
}

// ── Provider selection ───────────────────────────────────────────────────────────

async fn pick_ollama(state: &AppState) -> Result<LlmProvider, Response> {
    let providers = state
        .provider_registry
        .list_active()
        .await
        .map_err(|e| {
            tracing::error!("ollama_compat: provider list failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to list providers"})),
            )
                .into_response()
        })?;

    providers
        .into_iter()
        .find(|b| b.provider_type == ProviderType::Ollama)
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "no active Ollama provider"})),
            )
                .into_response()
        })
}
