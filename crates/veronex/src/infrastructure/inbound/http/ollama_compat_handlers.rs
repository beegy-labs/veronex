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
use tracing::instrument;

use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{ApiFormat, ProviderType};
use super::cancel_guard::CancelOnDrop;
use super::constants::{ERR_MODEL_INVALID, ERR_PROMPT_TOO_LARGE};
use super::handlers::sanitize_sse_error;
use super::inference_helpers::{validate_model_name, validate_content_length, extract_last_user_prompt, extract_conversation_id};
use super::inference_helpers::{validate_and_compress_images, analyze_images_for_context};
use super::middleware::infer_auth::InferCaller;
use super::state::AppState;

/// Collected output from a non-streaming token stream.
struct CollectedStream {
    content: String,
    tool_calls: Option<serde_json::Value>,
    prompt_tokens: u32,
    eval_tokens: u32,
}

/// Drain a token stream into collected output. Returns error response on failure.
async fn collect_stream(state: &AppState, job_id: &crate::domain::value_objects::JobId) -> Result<CollectedStream, Response> {
    let mut content = String::new();
    let mut tool_calls: Option<serde_json::Value> = None;
    let mut prompt_tokens = 0u32;
    let mut eval_tokens = 0u32;

    let mut stream = state.use_case.stream(job_id);
    while let Some(result) = stream.next().await {
        match result {
            Ok(t) if t.tool_calls.is_some() => tool_calls = t.tool_calls,
            Ok(t) if t.is_final => {
                prompt_tokens = t.prompt_tokens.unwrap_or(0);
                eval_tokens = t.completion_tokens.unwrap_or(0);
            }
            Ok(t) => content.push_str(&t.value),
            Err(e) => return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": sanitize_sse_error(&e)})),
            ).into_response()),
        }
    }
    Ok(CollectedStream { content, tool_calls, prompt_tokens, eval_tokens })
}

// ── Inference request body types ────────────────────────────────────────────────

/// Ollama `/api/generate` request body. Fields are read by serde but not
/// accessed directly in Rust code — `dead_code` is expected.
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
    /// `false` → collect all tokens and return a single JSON response.
    /// `true` or absent → default NDJSON streaming.
    #[serde(default)]
    stream: Option<bool>,
}

/// Ollama `/api/chat` request body. Fields read by serde, not accessed directly.
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
    /// `false` → collect all tokens and return a single JSON response.
    /// `true` or absent → default NDJSON streaming.
    #[serde(default)]
    stream: Option<bool>,
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
#[instrument(skip(state, req, headers), fields(model = %req.model))]
pub async fn generate(
    State(state): State<AppState>,
    axum::extract::Extension(caller): axum::extract::Extension<InferCaller>,
    headers: axum::http::HeaderMap,
    Json(mut req): Json<OllamaGenerateBody>,
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

    // Validate + compress oversized images, then analyze non-vision images.
    let mut vision_analysis = None;
    if req.images.is_some() {
        let lab = state.lab_settings_repo.get().await.unwrap_or_default();
        if let Some(msg) = validate_and_compress_images(&mut req.images, &lab).await {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response();
        }
        // For non-vision models: analyze images via vision model, inject description into prompt.
        // Images are kept in req.images for S3 upload and conversation history.
        if let Some(imgs) = req.images.as_deref().filter(|i| !i.is_empty()) {
            if let Some(va) = analyze_images_for_context(
                &state.http_client,
                state.provider_registry.as_ref(),
                &req.model,
                imgs,
                &req.prompt,
                lab.vision_model.as_deref(),
            ).await {
                req.prompt = format!("[Image Analysis]\n{}\n\n{}", va.analysis, req.prompt);
                vision_analysis = Some(va);
            }
        }
    }

    let model = req.model.clone();

    let job_id = match state
        .use_case
        .submit(SubmitJobRequest {
            prompt: req.prompt,
            model_name: model.clone(),
            provider_type: ProviderType::Ollama,
            gemini_tier: None,
            api_key_id: caller.api_key_id(),
            account_id: caller.account_id(),
            source: caller.source(),
            api_format: ApiFormat::OllamaNative,
            messages: None,
            tools: None,
            request_path: Some("/api/generate".to_string()),
            conversation_id,
            key_tier: caller.key_tier(),
            images: req.images,
            stop: None, seed: None, response_format: None,
            frequency_penalty: None, presence_penalty: None, mcp_loop_id: None, max_tokens: None,
            vision_analysis,
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

    // ── Non-streaming path (`stream: false`) ────────────────────────────────
    if req.stream == Some(false) {
        let c = match collect_stream(&state, &job_id).await {
            Ok(c) => c,
            Err(resp) => return resp,
        };
        let created_at = chrono::Utc::now().to_rfc3339();
        return Json(build_generate_response(
            &model, &created_at, c.content, c.prompt_tokens, c.eval_tokens,
        )).into_response();
    }

    // ── Streaming path (default) ────────────────────────────────────────────
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
#[instrument(skip(state, req, headers), fields(model = %req.model))]
pub async fn chat(
    State(state): State<AppState>,
    axum::extract::Extension(caller): axum::extract::Extension<InferCaller>,
    headers: axum::http::HeaderMap,
    Json(mut req): Json<OllamaChatBody>,
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

    // ── Multi-turn eligibility gate ──────────────────────────────────────────
    // Fire when the client sends a multi-message conversation (conversation_id present
    // and more than one user message indicates a continuing session).
    if conversation_id.is_some() {
        let user_msg_count = req.messages.iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .count();
        if user_msg_count > 1 {
            use crate::application::use_cases::inference::context_assembler;
            let lab = state.lab_settings_repo.get().await.unwrap_or_default();
            let max_ctx: Option<u32> = if let Some(ref vk) = state.valkey_pool {
                use fred::prelude::*;
                let providers = state.provider_registry.list_active().await.unwrap_or_default();
                let mut found = None;
                for p in providers.iter().filter(|p| p.provider_type == ProviderType::Ollama) {
                    let ctx_key = crate::infrastructure::outbound::valkey_keys::ollama_model_ctx(p.id, &req.model);
                    if let Ok(Some(raw)) = vk.get::<Option<String>, _>(&ctx_key).await {
                        if let Some(ctx) = serde_json::from_str::<serde_json::Value>(&raw).ok()
                            .and_then(|v| v["configured_ctx"].as_u64().filter(|&n| n > 0))
                        {
                            found = Some(ctx as u32);
                            break;
                        }
                    }
                }
                found
            } else {
                None
            };
            if let Err(e) = context_assembler::check_multiturn_eligibility(&req.model, max_ctx, &lab) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": {
                            "message": e.to_string(),
                            "type": "invalid_request_error",
                            "code": e.code()
                        }
                    })),
                ).into_response();
            }
        }
    }

    // Phase 5: compress long input inline if it exceeds budget
    // Only applies when context_compression_enabled and we have a conversation
    if conversation_id.is_some() {
        use crate::application::use_cases::inference::{compression_router, context_compressor};

        let lab5 = state.lab_settings_repo.get().await.unwrap_or_default();
        if lab5.context_compression_enabled {
            let route: compression_router::CompressionRoute = compression_router::decide(state.provider_registry.as_ref(), &lab5).await;
            if let Some(params) = route.into_params(
                lab5.compression_model.clone().unwrap_or_else(|| "qwen2.5:3b".to_string()),
                lab5.compression_timeout_secs as u64,
            ) {
                let configured_ctx = 32_768u32; // fallback; real value looked up during inference
                let input_budget = (configured_ctx as f32 * lab5.context_budget_ratio * 0.5) as u32;
                if let Some(compressed_prompt) = context_compressor::compress_input_inline(
                    &prompt,
                    input_budget,
                    &params.model,
                    &params.provider_url,
                    params.timeout_secs,
                ).await {
                    // Rewrite last user message with compressed prompt
                    if let Some(last_user) = req.messages.iter_mut().rev()
                        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    {
                        last_user["content"] = serde_json::json!(compressed_prompt);
                    }
                }
            }
        }
    }

    // Extract images from user messages (Ollama chat format: message-level `images` field).
    let mut images: Option<Vec<String>> = {
        let imgs: Vec<String> = req.messages.iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .filter_map(|m| m.get("images"))
            .filter_map(|v| v.as_array())
            .flat_map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)))
            .collect();
        if imgs.is_empty() { None } else { Some(imgs) }
    };

    // Validate + compress oversized images, then analyze non-vision images.
    let mut vision_analysis_chat = None;
    if images.is_some() {
        let lab = state.lab_settings_repo.get().await.unwrap_or_default();
        if let Some(msg) = validate_and_compress_images(&mut images, &lab).await {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg}))).into_response();
        }
        // For non-vision models: analyze images, inject description into last user message.
        if let Some(imgs) = images.as_deref().filter(|i| !i.is_empty()) {
            if let Some(va) = analyze_images_for_context(
                &state.http_client,
                state.provider_registry.as_ref(),
                &req.model,
                imgs,
                &prompt,
                lab.vision_model.as_deref(),
            ).await {
                if let Some(last_user) = req.messages.iter_mut().rev()
                    .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                {
                    let existing = last_user["content"].as_str().unwrap_or("").to_string();
                    last_user["content"] = serde_json::json!(format!("[Image Analysis]\n{}\n\n{existing}", va.analysis));
                }
                vision_analysis_chat = Some(va);
            }
        }
    }

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
            api_key_id: caller.api_key_id(),
            account_id: caller.account_id(),
            source: caller.source(),
            api_format: ApiFormat::OllamaNative,
            messages: Some(messages),
            tools,
            request_path: Some("/api/chat".to_string()),
            conversation_id,
            key_tier: caller.key_tier(),
            images,
            stop: None, seed: None, response_format: None,
            frequency_penalty: None, presence_penalty: None, mcp_loop_id: None, max_tokens: None,
            vision_analysis: vision_analysis_chat,
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

    // ── Non-streaming path (`stream: false`) ────────────────────────────────
    if req.stream == Some(false) {
        let c = match collect_stream(&state, &job_id).await {
            Ok(c) => c,
            Err(resp) => return resp,
        };
        let created_at = chrono::Utc::now().to_rfc3339();
        return Json(build_chat_response(
            &model, &created_at, c.content, c.tool_calls, c.prompt_tokens, c.eval_tokens,
        )).into_response();
    }

    // ── Streaming path (default) ────────────────────────────────────────────
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

// ── Non-streaming response builders ────────────────────────────────────────────
//
// Pure functions — no I/O, no async. Extracted for unit testability.

/// Build a single-shot `/api/chat` response for `stream: false`.
///
/// When `tool_calls` is `Some`, emits `done_reason: "tool_calls"` and
/// `message.content: ""` per the Ollama spec. Otherwise `done_reason: "stop"`.
fn build_chat_response(
    model: &str,
    created_at: &str,
    content: String,
    tool_calls: Option<serde_json::Value>,
    prompt_tokens: u32,
    eval_tokens: u32,
) -> serde_json::Value {
    let (done_reason, message) = if let Some(tc) = tool_calls {
        (
            "tool_calls",
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": tc}),
        )
    } else {
        (
            "stop",
            serde_json::json!({"role": "assistant", "content": content}),
        )
    };
    serde_json::json!({
        "model":               model,
        "created_at":          created_at,
        "message":             message,
        "done_reason":         done_reason,
        "done":                true,
        "total_duration":      0,
        "load_duration":       0,
        "prompt_eval_count":   prompt_tokens,
        "prompt_eval_duration":0,
        "eval_count":          eval_tokens,
        "eval_duration":       0,
    })
}

/// Build a single-shot `/api/generate` response for `stream: false`.
///
/// Uses `response` (not `message`) per the Ollama generate spec.
/// Timing fields are fixed at 0 — Veronex does not measure them.
fn build_generate_response(
    model: &str,
    created_at: &str,
    content: String,
    prompt_tokens: u32,
    eval_tokens: u32,
) -> serde_json::Value {
    serde_json::json!({
        "model":               model,
        "created_at":          created_at,
        "response":            content,
        "done_reason":         "stop",
        "done":                true,
        "total_duration":      0,
        "load_duration":       0,
        "prompt_eval_count":   prompt_tokens,
        "prompt_eval_duration":0,
        "eval_count":          eval_tokens,
        "eval_duration":       0,
    })
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

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. /api/chat stream:false — text response: done:true, message.content accumulated
    #[test]
    fn chat_response_text_content() {
        let resp = build_chat_response(
            "llama3.2",
            "2026-03-15T00:00:00Z",
            "Hello World".to_string(),
            None,
            10,
            5,
        );
        assert_eq!(resp["done"], true);
        assert_eq!(resp["done_reason"], "stop");
        assert_eq!(resp["message"]["role"], "assistant");
        assert_eq!(resp["message"]["content"], "Hello World");
        assert_eq!(resp["prompt_eval_count"], 10);
        assert_eq!(resp["eval_count"], 5);
    }

    // 2. /api/generate stream:false — response field populated
    #[test]
    fn generate_response_field() {
        let resp = build_generate_response(
            "llama3.2",
            "2026-03-15T00:00:00Z",
            "Paris".to_string(),
            8,
            3,
        );
        assert_eq!(resp["done"], true);
        assert_eq!(resp["done_reason"], "stop");
        assert_eq!(resp["response"], "Paris");
        assert!(resp.get("message").is_none(), "generate must not have 'message' field");
        assert_eq!(resp["prompt_eval_count"], 8);
        assert_eq!(resp["eval_count"], 3);
    }

    // 3. /api/chat stream:false + tool call — done_reason:"tool_calls", content:""
    #[test]
    fn chat_response_tool_calls() {
        let tc = serde_json::json!([{
            "function": {"name": "get_weather", "arguments": {"location": "Seoul"}}
        }]);
        let resp = build_chat_response(
            "llama3.2",
            "2026-03-15T00:00:00Z",
            String::new(),
            Some(tc.clone()),
            15,
            20,
        );
        assert_eq!(resp["done_reason"], "tool_calls");
        assert_eq!(resp["message"]["content"], "");
        assert_eq!(resp["message"]["tool_calls"], tc);
        assert_eq!(resp["done"], true);
    }

    // 4. Both response types include all required Ollama timing fields (all 0)
    #[test]
    fn response_has_timing_fields() {
        for resp in [
            build_generate_response("m", "t", String::new(), 0, 0),
            build_chat_response("m", "t", String::new(), None, 0, 0),
        ] {
            assert_eq!(resp["total_duration"], 0);
            assert_eq!(resp["load_duration"], 0);
            assert_eq!(resp["prompt_eval_duration"], 0);
            assert_eq!(resp["eval_duration"], 0);
        }
    }

    // 5. OllamaGenerateBody with images deserializes correctly
    #[test]
    fn generate_body_with_images() {
        let body: OllamaGenerateBody = serde_json::from_str(r#"{
            "model": "llava:7b",
            "prompt": "describe this",
            "images": ["abc123", "def456"]
        }"#).unwrap();
        assert_eq!(body.images.as_ref().unwrap().len(), 2);
        assert_eq!(body.images.as_ref().unwrap()[0], "abc123");
    }

    // 6. OllamaGenerateBody without images has None
    #[test]
    fn generate_body_without_images() {
        let body: OllamaGenerateBody = serde_json::from_str(r#"{
            "model": "llama3.2",
            "prompt": "hello"
        }"#).unwrap();
        assert!(body.images.is_none());
    }

    // 7. Response includes model name and created_at timestamp
    #[test]
    fn response_includes_model_and_timestamp() {
        let ts = "2026-03-15T12:00:00Z";
        let chat = build_chat_response("llava:7b", ts, String::new(), None, 0, 0);
        let generate = build_generate_response("llava:7b", ts, String::new(), 0, 0);
        for resp in [&chat, &generate] {
            assert_eq!(resp["model"], "llava:7b");
            assert_eq!(resp["created_at"], ts);
            assert_eq!(resp["done"], true);
        }
    }

}
