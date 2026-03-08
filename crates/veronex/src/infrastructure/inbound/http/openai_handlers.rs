use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;
use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::domain::enums::{ApiFormat, JobSource, ProviderType};
use super::constants::{ERR_MODEL_INVALID, ERR_PROMPT_TOO_LARGE, PROVIDER_OLLAMA, PROVIDER_GEMINI, GEMINI_TIER_FREE};
use super::handlers::sanitize_sse_error;
use super::inference_helpers::{build_sse_response, validate_model_name, validate_content_length, extract_last_user_prompt, validate_tool_call, extract_conversation_id};
use super::openai_sse_types::{
    ChatCompletion, CompletionChoice, CompletionChunk, CompletionMessage, UsageInfo,
};
use super::state::AppState;

// ── Request ────────────────────────────────────────────────────────────────────

/// OpenAI content can be a plain string or an array of content parts
/// (e.g. `[{"type":"text","text":"..."}]`). We normalise both to a String.
#[derive(Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Deserialize)]
struct ContentPart {
    #[serde(rename = "type")]
    part_type: String,
    text: Option<String>,
}

impl MessageContent {
    fn into_string(self) -> String {
        match self {
            MessageContent::Text(s) => s,
            MessageContent::Parts(parts) => parts
                .into_iter()
                .filter(|p| p.part_type == "text")
                .filter_map(|p| p.text)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    /// Content is optional — assistant tool-call messages may have no content.
    #[serde(default)]
    content: Option<MessageContent>,
    /// Tool calls requested by the assistant (OpenAI format).
    #[serde(default)]
    tool_calls: Option<serde_json::Value>,
    /// Tool result message correlation ID.
    #[serde(default)]
    tool_call_id: Option<String>,
    /// Tool result message name (some clients send this).
    #[serde(default)]
    name: Option<String>,
}

impl ChatMessage {
    pub fn content_str(self) -> String {
        match self.content {
            Some(c) => c.into_string(),
            None => String::new(),
        }
    }

    /// Convert to Ollama `/api/chat` message JSON.
    ///
    /// Key difference: OpenAI `tool_calls[].function.arguments` is a **JSON-encoded string**;
    /// Ollama expects it as a **JSON object**. We parse the string back to an object.
    /// The inverse applies for incoming tool result messages — no conversion needed there.
    fn into_ollama_value(self) -> serde_json::Value {
        let content = match self.content {
            Some(c) => c.into_string(),
            None => String::new(),
        };

        let mut msg = serde_json::json!({
            "role": self.role,
            "content": content,
        });

        // Pass tool name for tool-result messages (some Ollama versions use it).
        if let Some(name) = self.name {
            msg["name"] = serde_json::Value::String(name);
        }

        // Convert OpenAI tool_calls → Ollama tool_calls
        if let Some(serde_json::Value::Array(calls)) = self.tool_calls {
            let ollama_calls: Vec<serde_json::Value> = calls
                .into_iter()
                .map(|c| {
                    let name = c
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");
                    // OpenAI arguments is a JSON-encoded string; Ollama wants the object.
                    let arguments: serde_json::Value = c
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(serde_json::json!({}));
                    serde_json::json!({"function": {"name": name, "arguments": arguments}})
                })
                .collect();
            msg["tool_calls"] = serde_json::Value::Array(ollama_calls);
        }

        // Pass through tool_call_id for tool-result messages
        if let Some(id) = self.tool_call_id {
            msg["tool_call_id"] = serde_json::Value::String(id);
        }

        msg
    }
}

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// Selects the veronex provider type ("ollama" | "gemini"). Optional.
    pub provider_type: Option<String>,
    /// Tool/function definitions — passed through to Ollama as-is.
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    /// Tool choice override (passed through to Ollama).
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    /// Maps to Ollama `options.num_predict`.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response (SSE). Defaults to `false` per OpenAI spec.
    #[serde(default)]
    pub stream: Option<bool>,
}

// ── Handler ────────────────────────────────────────────────────────────────────

/// `POST /v1/chat/completions` — OpenAI-compatible chat endpoint.
///
/// For Ollama providers: proxies the full request (messages, tools, temperature, …)
/// directly to Ollama's `/api/chat` and streams the response in OpenAI SSE format,
/// including `tool_calls` deltas for function-calling agents.
///
/// For other providers: falls back to the legacy queue-based single-prompt path.
pub async fn chat_completions(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    if validate_model_name(&req.model).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": ERR_MODEL_INVALID}})),
        )
            .into_response();
    }

    // Validate total message content length (all messages combined).
    let total_content_len: usize = req.messages.iter().map(|m| {
        m.content.as_ref().map_or(0, |c| match c {
            MessageContent::Text(s) => s.len(),
            MessageContent::Parts(parts) => parts.iter().map(|p| p.text.as_ref().map_or(0, |t| t.len())).sum(),
        })
    }).sum();
    if validate_content_length(total_content_len).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": ERR_PROMPT_TOO_LARGE}})),
        )
            .into_response();
    }

    let conversation_id = extract_conversation_id(&headers);
    let stream = req.stream.unwrap_or(false);
    let provider_str = req.provider_type.as_deref().unwrap_or(PROVIDER_OLLAMA);
    match provider_str {
        PROVIDER_OLLAMA => ollama_chat_proxy(state, api_key, req, conversation_id, stream).await,
        _ => {
            // Parse "gemini-free" → (Gemini, Some("free")), "gemini" → (Gemini, None)
            let (provider_type, gemini_tier) = parse_provider_str(provider_str);
            legacy_queue_chat(state, api_key, req, provider_type, gemini_tier, conversation_id, stream).await
        }
    }
}

// ── Ollama queue-based path ─────────────────────────────────────────────────────

/// Routes an OpenAI chat request to an Ollama provider via the Veronex queue.
///
/// Messages are converted to Ollama `/api/chat` format and stored in the job
/// so the OllamaAdapter can forward the full conversation history.
/// VRAM availability and thermal throttle are checked before dispatch.
async fn ollama_chat_proxy(
    state: AppState,
    api_key: crate::domain::entities::ApiKey,
    req: ChatCompletionRequest,
    conversation_id: Option<String>,
    stream: bool,
) -> Response {
    // Convert messages to Ollama format (normalise content, convert tool_calls).
    let ollama_messages: Vec<serde_json::Value> =
        req.messages.into_iter().map(|m| m.into_ollama_value()).collect();

    // Extract last user content as display prompt (required by InferenceJob).
    let prompt = extract_last_user_prompt(&ollama_messages).to_string();

    let model = req.model.clone();
    let messages = serde_json::Value::Array(ollama_messages);
    // Forward tools in Ollama format (OpenAI tools array is already compatible with Ollama).
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
            api_format: ApiFormat::OpenaiCompat,
            messages: Some(messages),
            tools,
            request_path: Some("/v1/chat/completions".to_string()),
            conversation_id,
            key_tier: Some(api_key.tier),
        })
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("chat_completions(ollama): submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"message": "failed to submit inference job"}})),
            )
                .into_response();
        }
    };

    let chunk_id = format!("chatcmpl-{}", job_id.0);
    let created = chrono::Utc::now().timestamp();

    if !stream {
        return collect_completion(&state, job_id, model, chunk_id, created).await;
    }

    let mut saw_tool_calls = false;
    build_sse_response(&state, job_id, true, move |result| {
        match result {
            Ok(token) if token.tool_calls.is_some() => {
                saw_tool_calls = true;
                let ollama_calls = token.tool_calls.as_ref()
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let openai_calls: Vec<serde_json::Value> = ollama_calls
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| validate_tool_call(c))
                    .map(|(i, c)| convert_tool_call(i, c))
                    .collect();

                let chunk = CompletionChunk::tool_calls(chunk_id.clone(), created, Some(model.clone()), openai_calls);
                Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
            }
            Ok(token) if token.is_final => {
                let reason = if saw_tool_calls { "tool_calls" } else { "stop" };
                let chunk = CompletionChunk::finish(chunk_id.clone(), created, Some(model.clone()), reason);
                Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
            }
            Ok(token) => {
                if token.value.is_empty() {
                    return Event::default().data("");
                }
                let chunk = CompletionChunk::content(chunk_id.clone(), created, Some(model.clone()), token.value);
                Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": sanitize_sse_error(&e)}});
                Event::default().data(serde_json::to_string(&err).unwrap_or_default())
            }
        }
    })
}

/// Convert an Ollama tool call JSON value to OpenAI format.
fn convert_tool_call(i: usize, c: &serde_json::Value) -> serde_json::Value {
    let name = c.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = c.get("function")
        .and_then(|f| f.get("arguments"))
        .map(|a| serde_json::to_string(a).unwrap_or_default())
        .unwrap_or_default();
    serde_json::json!({
        "index": i,
        "id": format!("call_{i}"),
        "type": "function",
        "function": {"name": name, "arguments": args}
    })
}

// ── Non-streaming collection ──────────────────────────────────────────────────

/// Collect all tokens and return a non-streaming `ChatCompletion` response.
async fn collect_completion(
    state: &AppState,
    job_id: crate::domain::value_objects::JobId,
    model: String,
    id: String,
    created: i64,
) -> Response {
    let mut token_stream = state.use_case.stream(&job_id);
    let mut content = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut prompt_tokens: u32 = 0;
    let mut completion_tokens: u32 = 0;

    while let Some(result) = token_stream.next().await {
        match result {
            Ok(token) if token.tool_calls.is_some() => {
                if let Some(calls) = token.tool_calls.as_ref().and_then(|v| v.as_array()) {
                    for (i, c) in calls.iter().enumerate() {
                        if validate_tool_call(c) {
                            tool_calls.push(convert_tool_call(i, c));
                        }
                    }
                }
            }
            Ok(token) if token.is_final => {
                prompt_tokens = token.prompt_tokens.unwrap_or(0);
                completion_tokens = token.completion_tokens.unwrap_or(completion_tokens);
                break;
            }
            Ok(token) => {
                if !token.value.is_empty() {
                    content.push_str(&token.value);
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": {"message": sanitize_sse_error(&e)}})),
                )
                    .into_response();
            }
        }
    }

    let finish_reason = if tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    let total = prompt_tokens + completion_tokens;

    Json(ChatCompletion {
        id,
        object: "chat.completion",
        created,
        model,
        choices: vec![CompletionChoice {
            index: 0,
            message: CompletionMessage {
                role: "assistant",
                content: if content.is_empty() { None } else { Some(content) },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            },
            finish_reason: finish_reason.to_string(),
        }],
        usage: UsageInfo {
            prompt_tokens,
            completion_tokens,
            total_tokens: total,
        },
    })
    .into_response()
}

// ── Provider string parsing ──────────────────────────────────────────────────

/// Parse a provider type string from the HTTP request into `(ProviderType, Option<String>)`.
///
/// "gemini-free" → (Gemini, Some("free")), "gemini" → (Gemini, None), anything else → (Ollama, None).
fn parse_provider_str(s: &str) -> (ProviderType, Option<String>) {
    match s {
        "gemini-free" => (ProviderType::Gemini, Some(GEMINI_TIER_FREE.to_string())),
        PROVIDER_GEMINI => (ProviderType::Gemini, None),
        _ => (ProviderType::Ollama, None),
    }
}

// ── Legacy queue-based path (Gemini / other providers) ────────────────────────

async fn legacy_queue_chat(
    state: AppState,
    api_key: crate::domain::entities::ApiKey,
    req: ChatCompletionRequest,
    provider_type: ProviderType,
    gemini_tier: Option<String>,
    conversation_id: Option<String>,
    stream: bool,
) -> Response {
    // Extract prompt from the last user message.
    let prompt = req
        .messages
        .into_iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content_str())
        .unwrap_or_default();

    if prompt.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "no user message found in messages array"}})),
        )
            .into_response();
    }

    let model = req.model.clone();

    let job_id = match state
        .use_case
        .submit(SubmitJobRequest {
            prompt,
            model_name: model.clone(),
            provider_type,
            gemini_tier,
            api_key_id: Some(api_key.id),
            account_id: None,
            source: JobSource::Api,
            api_format: ApiFormat::OpenaiCompat,
            messages: None,
            tools: None,
            request_path: Some("/v1/chat/completions".to_string()),
            conversation_id,
            key_tier: Some(api_key.tier),
        })
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("chat_completions: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"message": "failed to submit inference job"}})),
            )
                .into_response();
        }
    };

    let chunk_id = format!("chatcmpl-{}", job_id.0);
    let created = chrono::Utc::now().timestamp();

    if !stream {
        return collect_completion(&state, job_id, model, chunk_id, created).await;
    }

    build_sse_response(&state, job_id, true, move |result| {
        match result {
            Ok(token) if token.is_final => {
                let chunk = CompletionChunk::stop(chunk_id.clone(), created, Some(model.clone()));
                Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
            }
            Ok(token) if token.value.is_empty() => Event::default().data(""),
            Ok(token) => {
                let chunk = CompletionChunk::content(chunk_id.clone(), created, Some(model.clone()), token.value);
                Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": sanitize_sse_error(&e)}});
                Event::default().data(serde_json::to_string(&err).unwrap_or_default())
            }
        }
    })
}
