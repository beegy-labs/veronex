use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use crate::domain::enums::{ApiFormat, JobSource};
use super::state::AppState;

type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

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
                        .unwrap_or("")
                        .to_string();
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
    /// Selects the veronex backend type ("ollama" | "gemini"). Optional.
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
    /// Ignored — the endpoint always streams (SSE).
    #[serde(default)]
    pub stream: Option<bool>,
}

// ── OpenAI SSE response types ───────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct DeltaContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize)]
struct ChunkChoice {
    index: u32,
    delta: DeltaContent,
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct CompletionChunk {
    id: String,
    object: &'static str,
    created: i64,
    model: String,
    choices: Vec<ChunkChoice>,
}

// ── Handler ────────────────────────────────────────────────────────────────────

/// `POST /v1/chat/completions` — OpenAI-compatible streaming chat endpoint.
///
/// For Ollama backends: proxies the full request (messages, tools, temperature, …)
/// directly to Ollama's `/api/chat` and streams the response in OpenAI SSE format,
/// including `tool_calls` deltas for function-calling agents.
///
/// For other backends: falls back to the legacy queue-based single-prompt path.
pub async fn chat_completions(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    let conversation_id = headers
        .get("x-conversation-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let backend_type = req.provider_type.clone().unwrap_or_else(|| "ollama".to_string());
    match backend_type.as_str() {
        "ollama" => ollama_chat_proxy(state, api_key, req, conversation_id).await,
        _ => legacy_queue_chat(state, api_key, req, backend_type, conversation_id).await,
    }
}

// ── Ollama queue-based path ─────────────────────────────────────────────────────

/// Routes an OpenAI chat request to an Ollama backend via the Veronex queue.
///
/// Messages are converted to Ollama `/api/chat` format and stored in the job
/// so the OllamaAdapter can forward the full conversation history.
/// VRAM availability and thermal throttle are checked before dispatch.
async fn ollama_chat_proxy(
    state: AppState,
    api_key: crate::domain::entities::ApiKey,
    req: ChatCompletionRequest,
    conversation_id: Option<String>,
) -> Response {
    // Convert messages to Ollama format (normalise content, convert tool_calls).
    let ollama_messages: Vec<serde_json::Value> =
        req.messages.into_iter().map(|m| m.into_ollama_value()).collect();

    // Extract last user content as display prompt (required by InferenceJob).
    let prompt = ollama_messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("chat")
        .to_string();

    let model = req.model.clone();
    let messages = serde_json::Value::Array(ollama_messages);
    // Forward tools in Ollama format (OpenAI tools array is already compatible with Ollama).
    let tools = req.tools.map(serde_json::Value::Array);

    let job_id = match state
        .use_case
        .submit(
            &prompt,
            &model,
            "ollama",
            Some(api_key.id),
            None,
            JobSource::Api,
            ApiFormat::OpenaiCompat,
            Some(messages),
            tools,
            Some("/v1/chat/completions".to_string()),
            conversation_id,
            Some(api_key.tier.clone()),
        )
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
    let token_stream = state.use_case.stream(&job_id);

    let mut saw_tool_calls = false;
    let content_stream = token_stream.map(move |result| -> Result<Event, Infallible> {
        match result {
            Ok(token) if token.tool_calls.is_some() => {
                // Model returned tool calls — emit OpenAI delta format.
                // Ollama format: [{function: {name, arguments: Object}}]
                // OpenAI format: [{index, id, type, function: {name, arguments: String}}]
                saw_tool_calls = true;
                let ollama_calls = token.tool_calls.as_ref()
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let openai_calls: Vec<serde_json::Value> = ollama_calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let name = c.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
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
                    })
                    .collect();

                let chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent {
                            role: None,
                            content: None,
                            tool_calls: Some(openai_calls),
                        },
                        finish_reason: None,
                    }],
                };
                Ok(Event::default().data(serde_json::to_string(&chunk).unwrap_or_default()))
            }
            Ok(token) if token.is_final => {
                let finish_reason = if saw_tool_calls { "tool_calls" } else { "stop" };
                let stop_chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent::default(),
                        finish_reason: Some(finish_reason.to_string()),
                    }],
                };
                Ok(Event::default().data(serde_json::to_string(&stop_chunk).unwrap_or_default()))
            }
            Ok(token) => {
                if token.value.is_empty() {
                    // Skip empty non-final, non-tool-call tokens
                    return Ok(Event::default().data(""));
                }
                let chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent {
                            role: None,
                            content: Some(token.value),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                };
                Ok(Event::default().data(serde_json::to_string(&chunk).unwrap_or_default()))
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": e.to_string()}});
                Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()))
            }
        }
    });

    let done_stream =
        futures::stream::once(async { Ok::<_, Infallible>(Event::default().data("[DONE]")) });

    let sse_stream: SseStream = Box::pin(content_stream.chain(done_stream));

    (
        [("X-Accel-Buffering", "no")],
        Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
        .into_response()
}

// ── Legacy queue-based path (Gemini / other backends) ─────────────────────────

async fn legacy_queue_chat(
    state: AppState,
    api_key: crate::domain::entities::ApiKey,
    req: ChatCompletionRequest,
    backend_type: String,
    conversation_id: Option<String>,
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
        .submit(&prompt, &model, &backend_type, Some(api_key.id), None, JobSource::Api, ApiFormat::OpenaiCompat, None, None, Some("/v1/chat/completions".to_string()), conversation_id, Some(api_key.tier.clone()))
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
    let token_stream = state.use_case.stream(&job_id);

    let content_stream = token_stream.map(move |result| -> Result<Event, Infallible> {
        match result {
            Ok(token) if token.is_final => {
                let stop_chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent::default(),
                        finish_reason: Some("stop".to_string()),
                    }],
                };
                Ok(Event::default().data(serde_json::to_string(&stop_chunk).unwrap_or_default()))
            }
            Ok(token) if token.value.is_empty() => Ok(Event::default().data("")),
            Ok(token) => {
                let chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent {
                            role: None,
                            content: Some(token.value),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                };
                Ok(Event::default().data(serde_json::to_string(&chunk).unwrap_or_default()))
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": e.to_string()}});
                Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()))
            }
        }
    });

    let done_stream =
        futures::stream::once(async { Ok::<_, Infallible>(Event::default().data("[DONE]")) });

    let sse_stream: SseStream = Box::pin(content_stream.chain(done_stream));

    (
        [("X-Accel-Buffering", "no")],
        Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
        .into_response()
}
