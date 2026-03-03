/// Test Run endpoints — JWT auth, no API key, no rate limit.
///
/// These handlers are used by the "Test Run" tab in the web dashboard and
/// by native API clients pointed at the test paths.
///
/// Jobs submitted here have `api_key_id = NULL` and `account_id = claims.sub`,
/// so they are excluded from API usage/performance metrics but are visible
/// in the Jobs dashboard with `source = 'test'`.
///
/// # Test paths by API format
///
/// | Format         | Path                                | Response format  |
/// |----------------|-------------------------------------|------------------|
/// | OpenAI-compat  | `POST /v1/test/completions`         | OpenAI SSE       |
/// | Ollama native  | `POST /v1/test/api/chat`            | Ollama NDJSON    |
/// | Ollama native  | `POST /v1/test/api/generate`        | Ollama NDJSON    |
/// | Gemini native  | `POST /v1/test/v1beta/models/{path}`| Gemini SSE       |
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{Response as HttpResponse, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{Stream, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use crate::domain::enums::{ApiFormat, JobSource};
use crate::domain::value_objects::JobId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;

use super::cancel_guard::CancelOnDrop;
use super::state::AppState;

type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

// ── Shared request types ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TestCompletionRequest {
    pub model: String,
    pub messages: Vec<TestChatMessage>,
    pub provider_type: Option<String>,
}

#[derive(Deserialize)]
pub struct TestChatMessage {
    pub role: String,
    pub content: String,
}

// ── OpenAI SSE response types ──────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct DeltaContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(serde::Serialize)]
struct ChunkChoice {
    index: u32,
    delta: DeltaContent,
    finish_reason: Option<&'static str>,
}

#[derive(serde::Serialize)]
struct CompletionChunk {
    id: String,
    object: &'static str,
    created: i64,
    model: String,
    choices: Vec<ChunkChoice>,
}

// ── OpenAI-compat test handler ─────────────────────────────────────────────────

/// `POST /v1/test/completions` — JWT-authenticated test run (OpenAI SSE format).
///
/// - `api_key_id = None` (no rate limiting, not counted in API metrics)
/// - `account_id = claims.sub` (tracks who ran the test)
/// - `source = JobSource::Test`, `api_format = ApiFormat::OpenaiCompat`
pub async fn test_completions(
    State(state): State<AppState>,
    axum::extract::Extension(claims): axum::extract::Extension<Claims>,
    Json(req): Json<TestCompletionRequest>,
) -> Response {
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

    let backend_type = req.provider_type.as_deref().unwrap_or("ollama").to_string();
    let model = req.model.clone();
    let account_id = Some(claims.sub);

    let job_id = match state
        .use_case
        .submit(&prompt, &model, &backend_type, None, account_id, JobSource::Test, ApiFormat::OpenaiCompat, None, None, Some("/v1/test/completions".to_string()), None, None)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("test_completions: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"message": "failed to submit test job"}})),
            )
                .into_response();
        }
    };

    stream_as_openai_sse(state, job_id, model)
}

fn stream_as_openai_sse(state: AppState, job_id: JobId, model: String) -> Response {
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
                        delta: DeltaContent { content: None },
                        finish_reason: Some("stop"),
                    }],
                };
                Ok(Event::default().data(serde_json::to_string(&stop_chunk).unwrap_or_default()))
            }
            Ok(token) => {
                let chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: DeltaContent { content: Some(token.value) },
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

    let sse_stream: SseStream = Box::pin(CancelOnDrop::new(
        content_stream.chain(done_stream),
        job_id,
        state.use_case.clone(),
    ));

    (
        [("X-Accel-Buffering", "no")],
        Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
        .into_response()
}

/// `GET /v1/test/jobs/{job_id}/stream` — JWT-authenticated SSE reconnect for test jobs.
pub async fn stream_test_job(
    Path(job_id): Path<Uuid>,
    State(state): State<AppState>,
    axum::extract::Extension(_claims): axum::extract::Extension<Claims>,
) -> impl IntoResponse {
    let jid = JobId(job_id);
    let chunk_id = format!("chatcmpl-{}", job_id);
    let created = chrono::Utc::now().timestamp();
    let token_stream = state.use_case.stream(&jid);

    let content_stream = token_stream.map(move |result| -> Result<Event, Infallible> {
        match result {
            Ok(token) if token.is_final => {
                let stop_chunk = serde_json::json!({
                    "id": chunk_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                });
                Ok(Event::default().data(stop_chunk.to_string()))
            }
            Ok(token) => {
                let chunk = serde_json::json!({
                    "id": chunk_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "choices": [{"index": 0, "delta": {"content": token.value}, "finish_reason": null}]
                });
                Ok(Event::default().data(chunk.to_string()))
            }
            Err(e) => {
                let err = serde_json::json!({"error": {"message": e.to_string()}});
                Ok(Event::default().data(err.to_string()))
            }
        }
    });

    let done_stream =
        futures::stream::once(async { Ok::<_, Infallible>(Event::default().data("[DONE]")) });

    let sse_stream: SseStream = Box::pin(CancelOnDrop::new(
        content_stream.chain(done_stream),
        jid,
        state.use_case.clone(),
    ));

    (
        [("X-Accel-Buffering", "no")],
        axum::response::sse::Sse::new(sse_stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
}

// ── Ollama native test handlers ────────────────────────────────────────────────

/// Request body for Ollama `/api/chat`.
#[derive(Deserialize)]
pub struct OllamaChatTestRequest {
    pub model: String,
    pub messages: Vec<OllamaMessage>,
    #[serde(default)]
    pub stream: Option<bool>,
}

/// Request body for Ollama `/api/generate`.
#[derive(Deserialize)]
pub struct OllamaGenerateTestRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub stream: Option<bool>,
}

#[derive(Deserialize)]
pub struct OllamaMessage {
    pub role: String,
    pub content: String,
}

/// `POST /v1/test/api/chat` — JWT-authenticated Ollama-format test run.
///
/// Accepts Ollama's `/api/chat` request body and streams the response as
/// Ollama NDJSON (`application/x-ndjson`).
///
/// Set `OLLAMA_HOST=http://veronex:3001` on the client, then point to `/v1/test/api/chat`
/// or use the Ollama CLI with `--host http://veronex:3001`.
pub async fn test_ollama_chat(
    State(state): State<AppState>,
    axum::extract::Extension(claims): axum::extract::Extension<Claims>,
    Json(req): Json<OllamaChatTestRequest>,
) -> Response {
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
            Json(serde_json::json!({"error": "no user message found"})),
        )
            .into_response();
    }

    let model = req.model.clone();

    let job_id = match state
        .use_case
        .submit(&prompt, &model, "ollama", None, Some(claims.sub), JobSource::Test, ApiFormat::OllamaNative, None, None, Some("/v1/test/api/chat".to_string()), None, None)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("test_ollama_chat: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to submit test job"})),
            )
                .into_response();
        }
    };

    stream_as_ollama_chat_ndjson(state, job_id, model)
}

/// `POST /v1/test/api/generate` — JWT-authenticated Ollama-format test run.
///
/// Accepts Ollama's `/api/generate` request body and streams the response as
/// Ollama NDJSON (`application/x-ndjson`).
pub async fn test_ollama_generate(
    State(state): State<AppState>,
    axum::extract::Extension(claims): axum::extract::Extension<Claims>,
    Json(req): Json<OllamaGenerateTestRequest>,
) -> Response {
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
        .submit(&req.prompt, &model, "ollama", None, Some(claims.sub), JobSource::Test, ApiFormat::OllamaNative, None, None, Some("/v1/test/api/generate".to_string()), None, None)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("test_ollama_generate: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to submit test job"})),
            )
                .into_response();
        }
    };

    stream_as_ollama_generate_ndjson(state, job_id, model)
}

/// Stream a queued job's tokens as Ollama `/api/chat` NDJSON format.
fn stream_as_ollama_chat_ndjson(state: AppState, job_id: JobId, model: String) -> Response {
    let token_stream = state.use_case.stream(&job_id);
    let model_clone = model.clone();

    let ndjson = token_stream.map(move |result| {
        let model = model_clone.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let line = match result {
            Ok(token) if token.is_final => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "message": {"role": "assistant", "content": ""},
                "done": true,
                "done_reason": "stop",
                "total_duration": 0,
                "eval_count": 0,
            }),
            Ok(token) => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "message": {"role": "assistant", "content": token.value},
                "done": false,
            }),
            Err(e) => serde_json::json!({
                "error": e.to_string(),
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

/// Stream a queued job's tokens as Ollama `/api/generate` NDJSON format.
fn stream_as_ollama_generate_ndjson(state: AppState, job_id: JobId, model: String) -> Response {
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
                "eval_count": 0,
            }),
            Ok(token) => serde_json::json!({
                "model": model,
                "created_at": created_at,
                "response": token.value,
                "done": false,
            }),
            Err(e) => serde_json::json!({
                "error": e.to_string(),
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

// ── Gemini native test handler ─────────────────────────────────────────────────

/// Request body for Gemini `/v1beta/models/{model}:generateContent`.
#[derive(Deserialize)]
pub struct GeminiTestRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(default)]
    pub generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Deserialize)]
pub struct GeminiContent {
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
pub struct GeminiPart {
    pub text: Option<String>,
}

#[derive(Deserialize)]
pub struct GeminiGenerationConfig {
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f64>,
}

/// `POST /v1/test/v1beta/models/{*path}` — JWT-authenticated Gemini-format test run.
///
/// Accepts Google Gemini API request body and streams the response as Gemini SSE
/// (`text/event-stream`).
///
/// Configure the Gemini CLI:
/// ```text
/// GOOGLE_GEMINI_BASE_URL=http://veronex:3001  (but for test path, use a custom adapter)
/// ```
pub async fn test_gemini_request(
    Path(path): Path<String>,
    State(state): State<AppState>,
    axum::extract::Extension(claims): axum::extract::Extension<Claims>,
    Json(req): Json<GeminiTestRequest>,
) -> Response {
    // Extract model name from path: "modelname:generateContent" or "modelname:streamGenerateContent"
    let (model, _action) = path
        .rsplitn(2, ':')
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .split_first()
        .map(|(action, rest)| (rest.join(":"), action.to_string()))
        .unwrap_or_else(|| (path.clone(), "generateContent".to_string()));

    // Extract last user text from Gemini contents
    let prompt = req
        .contents
        .iter()
        .rev()
        .find(|c| c.role.as_deref().unwrap_or("user") == "user")
        .and_then(|c| c.parts.first())
        .and_then(|p| p.text.as_deref())
        .unwrap_or("")
        .to_string();

    if prompt.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": {"message": "no user text found in contents"}})),
        )
            .into_response();
    }

    let job_id = match state
        .use_case
        .submit(&prompt, &model, "ollama", None, Some(claims.sub), JobSource::Test, ApiFormat::GeminiNative, None, None, Some("/v1/test/v1beta/models".to_string()), None, None)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("test_gemini_request: submit failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"message": "failed to submit test job"}})),
            )
                .into_response();
        }
    };

    stream_as_gemini_sse(state, job_id, model)
}

/// Stream a queued job's tokens as Gemini SSE format (`text/event-stream`).
fn stream_as_gemini_sse(state: AppState, job_id: JobId, model: String) -> Response {
    let token_stream = state.use_case.stream(&job_id);
    let model_clone = model.clone();

    let sse_bytes = token_stream.map(move |result| {
        let model = model_clone.clone();
        let data = match result {
            Ok(token) if token.is_final => serde_json::json!({
                "candidates": [{
                    "content": {"parts": [{"text": ""}], "role": "model"},
                    "finishReason": "STOP",
                    "index": 0,
                }],
                "modelVersion": model,
            }),
            Ok(token) => serde_json::json!({
                "candidates": [{
                    "content": {"parts": [{"text": token.value}], "role": "model"},
                    "finishReason": "",
                    "index": 0,
                }],
                "modelVersion": model,
            }),
            Err(e) => serde_json::json!({
                "error": {"message": e.to_string(), "code": 500},
            }),
        };
        Ok::<_, std::convert::Infallible>(Bytes::from(
            format!("data: {}\r\n\r\n", data),
        ))
    });

    let guarded = CancelOnDrop::new(sse_bytes, job_id, state.use_case.clone());
    HttpResponse::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(guarded))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
