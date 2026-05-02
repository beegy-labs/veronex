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
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::domain::enums::{ApiFormat, FinishReason, ProviderType};
use super::constants::{ERR_MODEL_INVALID, ERR_PROMPT_TOO_LARGE};
use super::handlers::sanitize_sse_error;
use super::inference_helpers::{build_sse_response, validate_model_name, validate_content_length, extract_last_user_prompt, extract_conversation_id};
use super::middleware::infer_auth::InferCaller;
use super::state::AppState;

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
#[instrument(skip(state, req, headers), fields(path = %path))]
pub async fn handle_request(
    State(state): State<AppState>,
    axum::extract::Extension(caller): axum::extract::Extension<InferCaller>,
    headers: axum::http::HeaderMap,
    Path(path): Path<String>,
    Json(req): Json<GeminiGenerateRequest>,
) -> Response {
    let conversation_id = extract_conversation_id(&headers);

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
    let action = parts.next().unwrap_or("");
    let model = parts.next().unwrap_or("").to_string();

    if validate_model_name(&model).is_err() {
        return gemini_error(StatusCode::BAD_REQUEST, 400, "INVALID_ARGUMENT", ERR_MODEL_INVALID);
    }

    // Validate total content length across all parts.
    let total_content_len: usize = req.contents.iter().flat_map(|c| &c.parts).map(|p| match p {
        GeminiPart::Text { text } => text.len(),
        _ => 0,
    }).sum();
    if validate_content_length(total_content_len).is_err() {
        return gemini_error(StatusCode::BAD_REQUEST, 400, "INVALID_ARGUMENT", ERR_PROMPT_TOO_LARGE);
    }

    match action {
        "streamGenerateContent" => stream_generate(state, caller, model, req, conversation_id).await,
        "generateContent" => generate_content(state, caller, model, req, conversation_id).await,
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
    caller: InferCaller,
    model: String,
    req: GeminiGenerateRequest,
    conversation_id: Option<uuid::Uuid>,
) -> Response {
    let messages = contents_to_ollama(req.system_instruction, req.contents);

    let prompt = extract_last_user_prompt(&messages).to_string();

    let messages_json = serde_json::Value::Array(messages);
    // Tools already passed the lab gate in handle_request; convert here.
    let tools = gemini_tools_to_ollama(req.tools);

    let request_path = format!("/v1beta/models/{}:streamGenerateContent", model);
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
            api_format: ApiFormat::GeminiNative,
            messages: Some(messages_json),
            tools,
            request_path: Some(request_path),
            conversation_id,
            key_tier: caller.key_tier(),
            images: None,
            stop: None, seed: None, response_format: None,
            frequency_penalty: None, presence_penalty: None, mcp_loop_id: None, max_tokens: None,
            vision_analysis: None,
        })
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

    build_sse_response(&state, job_id, false, move |result| {
        match result {
            Ok(token) if token.is_final => {
                let resp = GeminiResponse {
                    candidates: vec![GeminiCandidate {
                        content: GeminiResponseContent { parts: vec![], role: "model" },
                        index: 0,
                        finish_reason: Some(to_gemini_finish_reason(token.finish_reason.as_deref())),
                    }],
                    usage_metadata: Some(GeminiUsageMetadata {
                        prompt_token_count: token.prompt_tokens.unwrap_or(0),
                        candidates_token_count: token.completion_tokens.unwrap_or(0),
                    }),
                };
                vec![Event::default().data(serde_json::to_string(&resp).unwrap_or_default())]
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
                vec![Event::default().data(serde_json::to_string(&resp).unwrap_or_default())]
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": sanitize_sse_error(&e)}});
                vec![Event::default().data(serde_json::to_string(&err).unwrap_or_default())]
            }
        }
    })
}

// ── Non-streaming ───────────────────────────────────────────────────────────────

/// Route a Gemini generateContent request through the Veronex queue.
///
/// Submits the job, collects all streamed tokens, and returns a single
/// Gemini-format JSON response.
async fn generate_content(
    state: AppState,
    caller: InferCaller,
    model: String,
    req: GeminiGenerateRequest,
    conversation_id: Option<uuid::Uuid>,
) -> Response {
    let messages = contents_to_ollama(req.system_instruction, req.contents);

    let prompt = extract_last_user_prompt(&messages).to_string();

    let messages_json = serde_json::Value::Array(messages);
    let tools = gemini_tools_to_ollama(req.tools);

    let request_path = format!("/v1beta/models/{}:generateContent", model);
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
            api_format: ApiFormat::GeminiNative,
            messages: Some(messages_json),
            tools,
            request_path: Some(request_path),
            conversation_id,
            key_tier: caller.key_tier(),
            images: None,
            stop: None, seed: None, response_format: None,
            frequency_penalty: None, presence_penalty: None, mcp_loop_id: None, max_tokens: None,
            vision_analysis: None,
        })
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
    let mut gemini_finish_reason = "STOP".to_string();

    while let Some(result) = token_stream.next().await {
        match result {
            Ok(token) if token.is_final => {
                prompt_tokens = token.prompt_tokens.unwrap_or(0);
                completion_tokens = token.completion_tokens.unwrap_or(0);
                gemini_finish_reason = to_gemini_finish_reason(token.finish_reason.as_deref());
                break;
            }
            Ok(token) => full_text.push_str(&token.value),
            Err(e) => {
                return gemini_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    500,
                    "INTERNAL",
                    &sanitize_sse_error(&e),
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
            finish_reason: Some(gemini_finish_reason),
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

/// Map a Veronex/Ollama finish reason string to the Gemini wire format (uppercase).
///
/// Gemini uses: `"STOP"`, `"MAX_TOKENS"`, `"CANCELLED"`, `"OTHER"`.
/// Ollama/Veronex uses: `"stop"`, `"length"`, `"cancelled"`, `"error"`.
fn to_gemini_finish_reason(reason: Option<&str>) -> String {
    match reason.unwrap_or(FinishReason::Stop.as_str()) {
        "length" => "MAX_TOKENS",
        "cancelled" => "CANCELLED",
        "error" => "OTHER",
        _ => "STOP",
    }
    .to_string()
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
#[instrument(skip(state))]
pub async fn list_models(State(state): State<AppState>) -> Response {
    // Gather all active Ollama providers
    let providers = match state.provider_registry.list_active().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("gemini_compat list_models: {e}");
            return gemini_error(StatusCode::INTERNAL_SERVER_ERROR, 500, "INTERNAL", "failed to list models");
        }
    };

    // Collect enabled models across all Ollama providers (deduplicated)
    let mut seen = std::collections::HashSet::new();
    let mut model_names: Vec<String> = Vec::new();

    for provider in providers.iter().filter(|p| p.is_ollama()) {
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
#[instrument(skip(state), fields(model = %model))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_gemini_finish_reason_mappings() {
        assert_eq!(to_gemini_finish_reason(Some("length")),    "MAX_TOKENS");
        assert_eq!(to_gemini_finish_reason(Some("cancelled")), "CANCELLED");
        assert_eq!(to_gemini_finish_reason(Some("error")),     "OTHER");
        assert_eq!(to_gemini_finish_reason(Some("stop")),      "STOP");
        assert_eq!(to_gemini_finish_reason(None),              "STOP");
        assert_eq!(to_gemini_finish_reason(Some("unknown_x")), "STOP");
    }

    #[test]
    fn extract_text_parts_joins_text_only() {
        let parts = vec![
            GeminiPart::Text { text: "hello ".to_string() },
            GeminiPart::Text { text: "world".to_string() },
        ];
        assert_eq!(extract_text_parts(&parts), "hello world");
    }

}
