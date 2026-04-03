use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::domain::enums::{ApiFormat, ProviderType};
use crate::domain::value_objects::{JobId, pub_id_encode};

use super::constants::{GEMINI_TIER_FREE, PROVIDER_GEMINI, PROVIDER_OLLAMA, SSE_KEEP_ALIVE, SSE_MAX_CONNECTIONS, SSE_TIMEOUT};
use super::middleware::infer_auth::InferCaller;
use super::error::AppError;
use super::openai_sse_types::CompletionChunk;
use super::state::AppState;

/// Type alias for a boxed SSE event stream.  Re-exported for use by sibling handler modules.
pub(super) type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

/// Sanitize error message for SSE output: strip internal details and escape CRLF.
///
/// Uses a whitelist approach — only known-safe error categories produce a
/// descriptive message. Everything else gets a generic "inference failed"
/// to prevent leaking internal implementation details to clients.
pub(super) fn sanitize_sse_error(e: &dyn std::fmt::Display) -> String {
    let msg = e.to_string();
    let safe = match &*msg {
        m if m.contains("database") || m.contains("sqlx") || m.contains("postgres") => "internal processing error",
        m if m.contains("reqwest") || m.contains("connect") || m.contains("timeout") => "provider communication error",
        m if m.contains("capacity") || m.contains("slot") => "service at capacity",
        m if m.contains("cancelled") || m.contains("canceled") => "request cancelled",
        m if m.contains("token") && m.contains("limit") => "token limit exceeded",
        _ => "inference failed",
    };
    // Escape CRLF to prevent SSE frame injection
    safe.replace('\r', "\\r").replace('\n', "\\n")
}

/// RAII guard that decrements the SSE connection counter on drop.
pub(super) struct SseDropGuard(pub(super) Arc<AtomicU32>);

impl Drop for SseDropGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

/// Wrap an SSE stream with a hard timeout. After `SSE_TIMEOUT` elapses from
/// the first poll, the stream emits a final "timeout" event and terminates.
pub(super) fn with_sse_timeout(stream: SseStream) -> SseStream {
    let deadline = tokio::time::Instant::now() + SSE_TIMEOUT;
    Box::pin(async_stream::stream! {
        tokio::pin!(stream);
        loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => {
                    yield Ok(Event::default().event("error").data("stream timeout"));
                    break;
                }
                item = futures::StreamExt::next(&mut stream) => {
                    match item {
                        Some(event) => yield event,
                        None => break,
                    }
                }
            }
        }
    })
}

/// Try to acquire an SSE connection slot. Returns 429 on exhaustion.
#[allow(clippy::result_large_err)]
pub(super) fn try_acquire_sse(counter: &Arc<AtomicU32>) -> Result<SseDropGuard, Response> {
    let prev = counter.fetch_add(1, Ordering::Acquire);
    if prev >= SSE_MAX_CONNECTIONS {
        counter.fetch_sub(1, Ordering::Release);
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "too many concurrent SSE connections"})),
        ).into_response());
    }
    Ok(SseDropGuard(counter.clone()))
}

/// Build a fully-formed SSE response with timeout, keep-alive, and `X-Accel-Buffering: no`.
pub(super) fn sse_response(stream: SseStream) -> Response {
    (
        [("X-Accel-Buffering", "no")],
        Sse::new(with_sse_timeout(stream)).keep_alive(KeepAlive::new().interval(SSE_KEEP_ALIVE)),
    ).into_response()
}

/// Attach `X-Conversation-Id` header to a response (no-op if `cid` is None).
pub(super) fn with_conversation_id(mut resp: Response, cid: Option<&uuid::Uuid>) -> Response {
    if let Some(id) = cid {
        use super::inference_helpers::to_public_id;
        if let Ok(val) = axum::http::HeaderValue::from_str(&to_public_id(id)) {
            resp.headers_mut().insert(
                axum::http::header::HeaderName::from_static("x-conversation-id"),
                val,
            );
        }
    }
    resp
}

/// Parse a UUID string, returning `AppError::BadRequest` on failure.
pub(super) fn parse_uuid(s: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(s).map_err(|_| AppError::BadRequest(format!("invalid UUID: {s}")))
}

/// Validate a username: non-empty, ≤64 chars, ASCII alphanumeric + `_` `.` `-`.
pub(super) fn validate_username(username: &str) -> Result<(), AppError> {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("username must not be empty".into()));
    }
    if trimmed.len() > 64 {
        return Err(AppError::BadRequest("username too long".into()));
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
        return Err(AppError::BadRequest(
            "username must contain only alphanumeric characters, underscores, dots, or hyphens".into(),
        ));
    }
    Ok(())
}

// ── Shared pagination params ────────────────────────────────────────────────

/// Common query params for paginated list endpoints: `?search=&page=N&limit=N`.
#[derive(Debug, Deserialize, Default)]
pub struct ListPageParams {
    pub search: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

// ── Request / Response types ───────────────────────────────────────

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub prompt: String,
    pub model: String,
    #[serde(default = "default_provider_type")]
    pub provider_type: String,
}

fn default_provider_type() -> String {
    PROVIDER_OLLAMA.to_string()
}

#[derive(Serialize, Deserialize)]
pub struct SubmitResponse {
    pub job_id: JobId,
}

#[derive(Serialize, Deserialize)]
pub struct StatusResponse {
    pub job_id: JobId,
    pub status: String,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /v1/inference - Submit a new inference request.
pub async fn submit_inference(
    State(state): State<AppState>,
    axum::extract::Extension(caller): axum::extract::Extension<InferCaller>,
    Json(req): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, AppError> {
    if let Err(e) = super::inference_helpers::validate_content_length(req.prompt.len()) {
        return Err(AppError::BadRequest(e.into()));
    }
    if let Err(e) = super::inference_helpers::validate_model_name(&req.model) {
        return Err(AppError::BadRequest(e.into()));
    }

    let (provider_type, gemini_tier) = match req.provider_type.as_str() {
        "gemini-free" => (ProviderType::Gemini, Some(GEMINI_TIER_FREE.to_string())),
        PROVIDER_GEMINI => (ProviderType::Gemini, None),
        _ => (ProviderType::Ollama, None),
    };

    let job_id = state
        .use_case
        .submit(SubmitJobRequest {
            prompt: req.prompt,
            model_name: req.model,
            provider_type,
            gemini_tier,
            api_key_id: caller.api_key_id(),
            account_id: caller.account_id(),
            source: caller.source(),
            api_format: ApiFormat::VeronexNative,
            messages: None,
            tools: None,
            request_path: Some("/v1/inference".to_string()),
            conversation_id: None,
            key_tier: caller.key_tier(),
            images: None,
            stop: None, seed: None, response_format: None,
            frequency_penalty: None, presence_penalty: None, mcp_loop_id: None, max_tokens: None,
            vision_analysis: None,
        })
        .await?;

    Ok(Json(SubmitResponse { job_id }))
}

/// GET /v1/inference/:job_id/stream - SSE token streaming.
pub async fn stream_inference(
    Path(jid): Path<JobId>,
    State(state): State<AppState>,
) -> Response {
    let guard = match try_acquire_sse(&state.sse_connections) {
        Ok(g) => g,
        Err(resp) => return resp,
    };
    let token_stream = state.use_case.stream(&jid);

    let sse_stream: SseStream = Box::pin(token_stream.map(move |result| {
        let _ = &guard; // hold guard alive for stream lifetime
        match result {
            Ok(token) => {
                if token.is_final {
                    Ok::<_, Infallible>(Event::default().event("done").data(""))
                } else {
                    Ok::<_, Infallible>(Event::default().event("token").data(token.value))
                }
            }
            Err(e) => {
                Ok::<_, Infallible>(Event::default().event("error").data(sanitize_sse_error(&e)))
            }
        }
    }));

    sse_response(sse_stream)
}

/// GET /v1/inference/:job_id/status - Get job status.
pub async fn get_status(
    Path(jid): Path<JobId>,
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, AppError> {
    let status = state
        .use_case
        .get_status(&jid)
        .await?;

    Ok(Json(StatusResponse {
        job_id: jid,
        status: status.as_str().to_string(),
    }))
}

/// GET /v1/jobs/:job_id/stream — OpenAI-format SSE replay for test reconnect.
///
/// Streams a job's tokens in the same OpenAI chunk format as `/v1/chat/completions`.
/// Completed jobs are replayed from the DB; in-progress jobs stream live tokens.
pub async fn stream_job_openai(
    Path(jid): Path<JobId>,
    State(state): State<AppState>,
    axum::extract::Extension(_caller): axum::extract::Extension<InferCaller>,
) -> Response {
    let guard = match try_acquire_sse(&state.sse_connections) {
        Ok(g) => g,
        Err(resp) => return resp,
    };

    let chunk_id = format!("chatcmpl-{}", pub_id_encode("job", jid.0));
    let created = chrono::Utc::now().timestamp();
    let token_stream = state.use_case.stream(&jid);

    let content_stream = token_stream.map(move |result| -> Result<Event, std::convert::Infallible> {
        let _ = &guard; // hold guard alive for stream lifetime
        match result {
            Ok(token) if token.is_final => {
                let chunk = CompletionChunk::stop(chunk_id.clone(), created, None);
                Ok(Event::default().data(serde_json::to_string(&chunk).unwrap_or_default()))
            }
            Ok(token) => {
                let chunk = CompletionChunk::content(chunk_id.clone(), created, None, token.value);
                Ok(Event::default().data(serde_json::to_string(&chunk).unwrap_or_default()))
            }
            Err(e) => {
                tracing::error!(job_id = %jid, "SSE stream error: {e:?}");
                let err = serde_json::json!({"error": {"message": "inference failed"}});
                Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()))
            }
        }
    });

    let done_stream = futures::stream::once(async {
        Ok::<_, std::convert::Infallible>(Event::default().data("[DONE]"))
    });
    let sse_stream: SseStream = Box::pin(content_stream.chain(done_stream));

    sse_response(sse_stream)
}

/// DELETE /v1/inference/:job_id - Cancel a job.
pub async fn cancel_inference(
    Path(jid): Path<JobId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    state.use_case.cancel(&jid).await?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use super::super::test_support::make_app;
    use crate::domain::value_objects::JobId;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    // ── submit_inference tests ─────────────────────────────────────

    #[tokio::test]
    async fn submit_valid_request_returns_200_with_job_id() {
        let app = make_app();
        let body = serde_json::json!({
            "prompt": "Hello world",
            "model": "llama3.2",
            "provider_type": "ollama"
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/inference")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = resp["job_id"].as_str().unwrap();
        // Verify it's a prefixed base62 job ID
        assert!(job_id.starts_with("job_"), "expected job_ prefix, got: {job_id}");
    }

    #[tokio::test]
    async fn submit_with_default_provider_type() {
        let app = make_app();
        let body = serde_json::json!({
            "prompt": "Hello world",
            "model": "llama3.2"
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/inference")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn submit_missing_fields_returns_422() {
        let app = make_app();
        let body = serde_json::json!({
            "prompt": "Hello world"
            // missing "model"
        });

        let request = Request::builder()
            .method("POST")
            .uri("/v1/inference")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // ── get_status tests ───────────────────────────────────────────

    #[tokio::test]
    async fn get_status_valid_job_id_returns_status() {
        let app = make_app();
        let jid = JobId::new();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/status", jid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: StatusResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.status, "running");
        assert_eq!(resp.job_id, jid);
    }

    #[tokio::test]
    async fn get_status_invalid_job_id_returns_400() {
        let app = make_app();

        let request = Request::builder()
            .method("GET")
            .uri("/v1/inference/not-a-valid-id/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        // Path deserialization failure returns 422 BAD_REQUEST
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ── cancel_inference tests ─────────────────────────────────────

    #[tokio::test]
    async fn cancel_valid_job_id_returns_200() {
        let app = make_app();
        let jid = JobId::new();

        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/inference/{}", jid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cancel_invalid_job_id_returns_400() {
        let app = make_app();

        let request = Request::builder()
            .method("DELETE")
            .uri("/v1/inference/not-a-valid-id")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ── stream_inference tests ─────────────────────────────────────

    #[tokio::test]
    async fn stream_returns_sse_content_type() {
        let app = make_app();
        let jid = JobId::new();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/stream", jid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("text/event-stream"),
            "expected text/event-stream, got: {}",
            content_type
        );
    }

    #[tokio::test]
    async fn stream_has_no_buffering_header() {
        let app = make_app();
        let jid = JobId::new();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/stream", jid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        let buffering = response
            .headers()
            .get("X-Accel-Buffering")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(buffering, "no");
    }

    #[tokio::test]
    async fn stream_contains_token_and_done_events() {
        let app = make_app();
        let jid = JobId::new();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/stream", jid))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);

        assert!(
            body_str.contains("event: token"),
            "expected token event in body: {}",
            body_str
        );
        assert!(
            body_str.contains("event: done"),
            "expected done event in body: {}",
            body_str
        );
        assert!(
            body_str.contains("data: Hello"),
            "expected Hello data in body: {}",
            body_str
        );
    }
}
