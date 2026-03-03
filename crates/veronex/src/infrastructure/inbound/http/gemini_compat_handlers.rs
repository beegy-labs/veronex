/// Gemini API-compatible gateway endpoint.
///
/// Allows Gemini CLI and any client using `GOOGLE_GEMINI_BASE_URL` to route
/// requests through Veronex to any active Ollama provider.
///
/// Supports:
/// - `POST /v1beta/models/{model}:streamGenerateContent`  (SSE streaming)
/// - `POST /v1beta/models/{model}:generateContent`        (non-streaming)
///
/// Format conversions performed transparently:
/// - Gemini `contents[]` ↔ Ollama `messages[]`  (role + part mapping)
/// - Gemini `functionDeclarations[]` ↔ Ollama `tools[]`
/// - Gemini `functionCall` / `functionResponse` ↔ Ollama `tool_calls` / `tool` messages
/// - Gemini `generationConfig` ↔ Ollama `options`
///
/// Auth: `x-goog-api-key` header (or standard `X-API-Key` / `Authorization: Bearer`).
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::domain::enums::{ApiFormat, JobSource};
use super::cancel_guard::CancelOnDrop;
use super::state::AppState;

type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

// ── Gemini API request types ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GeminiGenerateRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(default)]
    pub tools: Vec<GeminiTool>,
    #[serde(rename = "generationConfig", default)]
    pub generation_config: Option<GenerationConfig>,
    /// System-level instructions (prepended as Ollama system message).
    #[serde(rename = "systemInstruction", default)]
    pub system_instruction: Option<GeminiContent>,
}

#[derive(Deserialize)]
pub struct GeminiContent {
    #[serde(default)]
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

/// A single content part — text, a tool invocation, or a tool result.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum GeminiPart {
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
    Text {
        text: String,
    },
    /// Catch-all for unknown part types (images, etc.) — silently ignored.
    Other(serde_json::Value),
}

#[derive(Deserialize)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Deserialize)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Deserialize, Default)]
pub struct GeminiTool {
    #[serde(rename = "functionDeclarations", default)]
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Deserialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
pub struct GenerationConfig {
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens", default)]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "topP", default)]
    pub top_p: Option<f64>,
}

// ── Gemini API response types ───────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata", skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Serialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
    index: u32,
    #[serde(rename = "finishReason", skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
    role: &'static str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum GeminiResponsePart {
    Text { text: String },
}

#[derive(Serialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: u32,
}

// ── Handler ─────────────────────────────────────────────────────────────────────

/// `POST /v1beta/models/{*path}` — Gemini API-compatible gateway.
///
/// Configure Gemini CLI:
/// ```text
/// export GOOGLE_GEMINI_BASE_URL="http://localhost:3001"
/// export GEMINI_API_KEY="<veronex-api-key>"
/// export GEMINI_MODEL="qwen3-coder-next-128k:latest"
/// ```
///
/// **Function calling (tool use)** is a lab feature: disabled by default.
/// Enable it in Settings → Lab Features before sending requests with `tools`.
pub async fn handle_request(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    headers: axum::http::HeaderMap,
    Path(path): Path<String>,
    Json(req): Json<GeminiGenerateRequest>,
) -> Response {
    let conversation_id = headers.get("x-conversation-id").and_then(|v| v.to_str().ok()).map(str::to_string);

    // ── Lab: function calling gate ───────────────────────────────────
    // If the request carries tool declarations, check whether the
    // "Gemini function calling" lab feature is enabled.
    // Disabled → 501 with a descriptive message so developers know
    // exactly which setting to toggle.
    let has_tools = !req.tools.is_empty();
    if has_tools {
        let lab = state.lab_settings_repo.get().await.unwrap_or_default();
        if !lab.gemini_function_calling {
            return gemini_error(
                StatusCode::NOT_IMPLEMENTED,
                501,
                "UNIMPLEMENTED",
                "Gemini function calling is a lab (experimental) feature. \
                 Enable it in Settings → Lab Features → Gemini function calling.",
            );
        }
    }

    // Parse "qwen3-coder:latest:streamGenerateContent"
    //   → model  = "qwen3-coder:latest"
    //   → action = "streamGenerateContent"
    let mut parts = path.rsplitn(2, ':');
    let action = parts.next().unwrap_or("").to_string();
    let model = parts.next().unwrap_or("").to_string();

    if model.is_empty() {
        return gemini_error(StatusCode::BAD_REQUEST, 400, "INVALID_ARGUMENT", "invalid model path");
    }

    match action.as_str() {
        "streamGenerateContent" => stream_generate(state, api_key, model, req, conversation_id).await,
        "generateContent" => generate_content(state, api_key, model, req, conversation_id).await,
        _ => gemini_error(
            StatusCode::NOT_FOUND,
            404,
            "NOT_FOUND",
            &format!("unknown action: {action}"),
        ),
    }
}

// ── Streaming ───────────────────────────────────────────────────────────────────

/// Route a Gemini streamGenerateContent request through the Veronex queue.
///
/// Converts Gemini `contents[]` to Ollama `/api/chat` messages format,
/// submits the job via the queue for VRAM-aware dispatch, and streams
/// the response as Gemini SSE chunks.
async fn stream_generate(
    state: AppState,
    api_key: crate::domain::entities::ApiKey,
    model: String,
    req: GeminiGenerateRequest,
    conversation_id: Option<String>,
) -> Response {
    let messages = contents_to_ollama(req.system_instruction, req.contents);

    // Extract last user message as display prompt (required by InferenceJob).
    let prompt = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("chat")
        .to_string();

    let messages_json = serde_json::Value::Array(messages);
    // Tools already passed the lab gate in handle_request; convert here.
    let tools = gemini_tools_to_ollama(req.tools);

    let job_id = match state
        .use_case
        .submit(
            &prompt,
            &model,
            "ollama",
            Some(api_key.id),
            None,
            JobSource::Api,
            ApiFormat::GeminiNative,
            Some(messages_json),
            tools,
            Some(format!("/v1beta/models/{}:streamGenerateContent", model)),
            conversation_id,
            Some(api_key.tier.clone()),
        )
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("gemini_compat stream_generate: submit failed: {e}");
            return gemini_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "INTERNAL",
                "failed to submit inference job",
            );
        }
    };

    let token_stream = state.use_case.stream(&job_id);

    let sse_stream: SseStream = Box::pin(CancelOnDrop::new(token_stream.map(|result| -> Result<Event, Infallible> {
        match result {
            Ok(token) if token.is_final => {
                let resp = GeminiResponse {
                    candidates: vec![GeminiCandidate {
                        content: GeminiResponseContent { parts: vec![], role: "model" },
                        index: 0,
                        finish_reason: Some("STOP".to_string()),
                    }],
                    usage_metadata: Some(GeminiUsageMetadata {
                        prompt_token_count: token.prompt_tokens.unwrap_or(0),
                        candidates_token_count: token.completion_tokens.unwrap_or(0),
                    }),
                };
                Ok(Event::default().data(serde_json::to_string(&resp).unwrap_or_default()))
            }
            Ok(token) => {
                let resp = GeminiResponse {
                    candidates: vec![GeminiCandidate {
                        content: GeminiResponseContent {
                            parts: vec![GeminiResponsePart::Text { text: token.value }],
                            role: "model",
                        },
                        index: 0,
                        finish_reason: None,
                    }],
                    usage_metadata: None,
                };
                Ok(Event::default().data(serde_json::to_string(&resp).unwrap_or_default()))
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": e.to_string()}});
                Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()))
            }
        }
    }), job_id, state.use_case.clone()));

    (
        [("X-Accel-Buffering", "no")],
        Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
        .into_response()
}

// ── Non-streaming ───────────────────────────────────────────────────────────────

/// Route a Gemini generateContent request through the Veronex queue.
///
/// Submits the job, collects all streamed tokens, and returns a single
/// Gemini-format JSON response.
async fn generate_content(
    state: AppState,
    api_key: crate::domain::entities::ApiKey,
    model: String,
    req: GeminiGenerateRequest,
    conversation_id: Option<String>,
) -> Response {
    let messages = contents_to_ollama(req.system_instruction, req.contents);

    let prompt = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("chat")
        .to_string();

    let messages_json = serde_json::Value::Array(messages);
    let tools = gemini_tools_to_ollama(req.tools);

    let job_id = match state
        .use_case
        .submit(
            &prompt,
            &model,
            "ollama",
            Some(api_key.id),
            None,
            JobSource::Api,
            ApiFormat::GeminiNative,
            Some(messages_json),
            tools,
            Some(format!("/v1beta/models/{}:generateContent", model)),
            conversation_id,
            Some(api_key.tier.clone()),
        )
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("gemini_compat generate_content: submit failed: {e}");
            return gemini_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "INTERNAL",
                "failed to submit inference job",
            );
        }
    };

    let mut token_stream = state.use_case.stream(&job_id);
    let mut full_text = String::new();
    let mut prompt_tokens = 0u32;
    let mut completion_tokens = 0u32;

    while let Some(result) = token_stream.next().await {
        match result {
            Ok(token) if token.is_final => {
                prompt_tokens = token.prompt_tokens.unwrap_or(0);
                completion_tokens = token.completion_tokens.unwrap_or(0);
                break;
            }
            Ok(token) => full_text.push_str(&token.value),
            Err(e) => {
                return gemini_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    500,
                    "INTERNAL",
                    &e.to_string(),
                );
            }
        }
    }

    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiResponseContent {
                parts: vec![GeminiResponsePart::Text { text: full_text }],
                role: "model",
            },
            index: 0,
            finish_reason: Some("STOP".to_string()),
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: prompt_tokens,
            candidates_token_count: completion_tokens,
        }),
    };

    Json(resp).into_response()
}

// ── Format conversion helpers ───────────────────────────────────────────────────

/// Convert Gemini `contents` + optional `systemInstruction` → Ollama `messages`.
///
/// Role mapping:
/// - Gemini `user`  → Ollama `user`
/// - Gemini `model` → Ollama `assistant`
///
/// Part mapping:
/// - `text` parts → concatenated into `content` string
/// - `functionCall` parts → `tool_calls` array (Ollama object format)
/// - `functionResponse` parts → separate Ollama `tool` messages
fn contents_to_ollama(
    system: Option<GeminiContent>,
    contents: Vec<GeminiContent>,
) -> Vec<serde_json::Value> {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // System instruction → prepended system message
    if let Some(sys) = system {
        let text = extract_text_parts(&sys.parts);
        if !text.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": text}));
        }
    }

    for content in contents {
        let role = content.role.as_deref().unwrap_or("user");
        let ollama_role = if role == "model" { "assistant" } else { role };

        // Gemini encodes tool results as user-role messages with functionResponse parts.
        // Detect and convert them to Ollama tool messages.
        let all_fn_responses = content.parts.iter().all(|p| {
            matches!(p, GeminiPart::FunctionResponse { .. })
        });

        if all_fn_responses {
            for part in content.parts {
                if let GeminiPart::FunctionResponse { function_response } = part {
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "name": function_response.name,
                        "content": serde_json::to_string(&function_response.response).unwrap_or_default(),
                    }));
                }
            }
            continue;
        }

        // Normal message: collect text parts and function calls
        let mut texts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<serde_json::Value> = Vec::new();

        for part in content.parts {
            match part {
                GeminiPart::Text { text } => texts.push(text),
                GeminiPart::FunctionCall { function_call } => {
                    // Ollama tool_calls use object arguments (not JSON string)
                    tool_calls.push(serde_json::json!({
                        "function": {
                            "name": function_call.name,
                            "arguments": function_call.args,
                        }
                    }));
                }
                GeminiPart::FunctionResponse { function_response } => {
                    // Mixed message edge case: emit tool message inline
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "name": function_response.name,
                        "content": serde_json::to_string(&function_response.response).unwrap_or_default(),
                    }));
                }
                GeminiPart::Other(_) => {}
            }
        }

        let mut msg = serde_json::json!({
            "role": ollama_role,
            "content": texts.join(""),
        });
        if !tool_calls.is_empty() {
            msg["tool_calls"] = serde_json::Value::Array(tool_calls);
        }
        messages.push(msg);
    }

    messages
}

/// Convert Gemini `tools[].functionDeclarations` → Ollama `tools` JSON Value (Array).
///
/// Only called when the Gemini function-calling lab feature is enabled.
/// Returns `None` when the tools list is empty (no tools → submit without tools).
fn gemini_tools_to_ollama(tools: Vec<GeminiTool>) -> Option<serde_json::Value> {
    let ollama_tools: Vec<serde_json::Value> = tools
        .into_iter()
        .flat_map(|t| t.function_declarations)
        .map(|fd| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": fd.name,
                    "description": fd.description.unwrap_or_default(),
                    "parameters": fd.parameters.unwrap_or_else(|| serde_json::json!({})),
                }
            })
        })
        .collect();

    if ollama_tools.is_empty() { None } else { Some(serde_json::Value::Array(ollama_tools)) }
}

fn extract_text_parts(parts: &[GeminiPart]) -> String {
    parts
        .iter()
        .filter_map(|p| if let GeminiPart::Text { text } = p { Some(text.as_str()) } else { None })
        .collect::<Vec<_>>()
        .join("")
}

/// Build a Gemini-format error response.
fn gemini_error(http: StatusCode, code: u32, status: &str, message: &str) -> Response {
    (
        http,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "status": status,
            }
        })),
    )
        .into_response()
}

// ── Model listing ────────────────────────────────────────────────────────────────

/// `GET /v1beta/models` — list available models.
///
/// Returns only Ollama models that are explicitly **enabled** on at least one
/// active provider (via the model selection feature in the Providers page).
/// This is the "selected models" subset of the full synchronized model list.
pub async fn list_models(State(state): State<AppState>) -> Response {
    // Gather all active Ollama providers
    let backends = match state.provider_registry.list_active().await {
        Ok(b) => b,
        Err(e) => {
            return gemini_error(StatusCode::INTERNAL_SERVER_ERROR, 500, "INTERNAL", &e.to_string());
        }
    };

    // Collect enabled models across all Ollama providers (deduplicated)
    let mut seen = std::collections::HashSet::new();
    let mut model_names: Vec<String> = Vec::new();

    for provider in backends.iter().filter(|b| b.provider_type == crate::domain::enums::ProviderType::Ollama) {
        if let Ok(enabled) = state.model_selection_repo.list_enabled(provider.id).await {
            for name in enabled {
                if seen.insert(name.clone()) {
                    model_names.push(name);
                }
            }
        }
    }

    // If no models are selected yet, fall back to all synchronized models
    if model_names.is_empty() {
        model_names = state.ollama_model_repo.list_all().await.unwrap_or_default();
    }

    model_names.sort();

    let models: Vec<serde_json::Value> = model_names
        .into_iter()
        .map(|name| {
            serde_json::json!({
                "name":    format!("models/{name}"),
                "baseModelId": name,
                "displayName": name,
                "description": "",
                "inputTokenLimit":  128000,
                "outputTokenLimit": 8192,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"],
            })
        })
        .collect();

    Json(serde_json::json!({ "models": models })).into_response()
}

/// `GET /v1beta/models/{model}` — get info for a single model.
pub async fn get_model(State(state): State<AppState>, axum::extract::Path(model): axum::extract::Path<String>) -> Response {
    // Strip "models/" prefix if present (Gemini SDK adds it)
    let model_name = model.strip_prefix("models/").unwrap_or(&model);

    // Check if model exists in synchronized list
    let all = state.ollama_model_repo.list_all().await.unwrap_or_default();
    if !all.iter().any(|n| n == model_name) {
        return gemini_error(
            StatusCode::NOT_FOUND,
            404,
            "NOT_FOUND",
            &format!("model not found: {model_name}"),
        );
    }

    Json(serde_json::json!({
        "name":    format!("models/{model_name}"),
        "baseModelId": model_name,
        "displayName": model_name,
        "description": "",
        "inputTokenLimit":  128000,
        "outputTokenLimit": 8192,
        "supportedGenerationMethods": ["generateContent", "streamGenerateContent"],
    }))
    .into_response()
}
