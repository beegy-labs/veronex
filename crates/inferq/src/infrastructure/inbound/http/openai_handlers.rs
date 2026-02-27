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
use uuid::Uuid;

use super::state::AppState;

type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

// ── Request ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// Selects the inferq backend type ("ollama" | "gemini"). Optional.
    pub backend: Option<String>,
}

// ── Response chunk types ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct DeltaContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(Serialize)]
struct ChunkChoice {
    index: u32,
    delta: DeltaContent,
    finish_reason: Option<&'static str>,
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
/// Accepts a standard `messages` array, submits the last user message as an
/// inferq job, and streams the response as `text/event-stream` in the OpenAI
/// chunk format (`data: {...}\n\n` … `data: [DONE]\n\n`).
pub async fn chat_completions(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    // Extract prompt from the last user message.
    let prompt = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("")
        .to_string();

    if prompt.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "no user message found in messages array"}})),
        )
            .into_response();
    }

    let backend_type = req.backend.as_deref().unwrap_or("ollama").to_string();
    let model = req.model.clone();

    let job_id = match state
        .use_case
        .submit(&prompt, &model, &backend_type, Some(api_key.id))
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

    let chunk_id = format!("chatcmpl-{}", Uuid::now_v7());
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
                        delta: DeltaContent { role: None, content: None },
                        finish_reason: Some("stop"),
                    }],
                };
                Ok(Event::default()
                    .data(serde_json::to_string(&stop_chunk).unwrap_or_default()))
            }
            Ok(token) => {
                let chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent { role: None, content: Some(token.value) },
                        finish_reason: None,
                    }],
                };
                Ok(Event::default()
                    .data(serde_json::to_string(&chunk).unwrap_or_default()))
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": e.to_string()}});
                Ok(Event::default()
                    .data(serde_json::to_string(&err).unwrap_or_default()))
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
