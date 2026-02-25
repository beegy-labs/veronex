use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::value_objects::JobId;

use super::state::AppState;

/// Type alias for a boxed SSE event stream.
type SseStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

// ── Request / Response types ───────────────────────────────────────

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub prompt: String,
    pub model: String,
    #[serde(default = "default_backend")]
    pub backend: String,
}

fn default_backend() -> String {
    "ollama".to_string()
}

#[derive(Serialize, Deserialize)]
pub struct SubmitResponse {
    pub job_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct StatusResponse {
    pub job_id: String,
    pub status: String,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /v1/inference - Submit a new inference request.
pub async fn submit_inference(
    State(state): State<AppState>,
    axum::extract::Extension(api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
    Json(req): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, StatusCode> {
    let job_id = state
        .use_case
        .submit(&req.prompt, &req.model, &req.backend, Some(api_key.id))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(SubmitResponse {
        job_id: job_id.to_string(),
    }))
}

/// GET /v1/inference/:job_id/stream - SSE token streaming.
pub async fn stream_inference(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let sse_stream: SseStream = match Uuid::parse_str(&job_id) {
        Ok(uuid) => {
            let jid = JobId(uuid);
            let token_stream = state.use_case.stream(&jid);

            Box::pin(token_stream.map(|result| match result {
                Ok(token) => {
                    if token.is_final {
                        Ok::<_, Infallible>(Event::default().event("done").data(""))
                    } else {
                        Ok::<_, Infallible>(Event::default().event("token").data(token.value))
                    }
                }
                Err(e) => {
                    Ok::<_, Infallible>(Event::default().event("error").data(e.to_string()))
                }
            }))
        }
        Err(_) => Box::pin(futures::stream::once(async {
            Ok::<_, Infallible>(Event::default().event("error").data("invalid job_id format"))
        })),
    };

    (
        [("X-Accel-Buffering", "no")],
        Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
}

/// GET /v1/inference/:job_id/status - Get job status.
pub async fn get_status(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, StatusCode> {
    let uuid = Uuid::parse_str(&job_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let jid = JobId(uuid);

    let status = state
        .use_case
        .get_status(&jid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let status_str = serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{:?}", status).to_lowercase());

    Ok(Json(StatusResponse {
        job_id: job_id.to_string(),
        status: status_str,
    }))
}

/// DELETE /v1/inference/:job_id - Cancel a job.
pub async fn cancel_inference(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&job_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let jid = JobId(uuid);

    state
        .use_case
        .cancel(&jid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
    use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
    use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
    use crate::domain::entities::{ApiKey, LlmBackend};
    use crate::domain::enums::LlmBackendStatus;
    use crate::domain::enums::JobStatus;
    use crate::domain::value_objects::StreamToken;
    use crate::infrastructure::inbound::http::router;
    use anyhow::Result;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use futures::Stream;
    use std::pin::Pin;
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    // ── Mock InferenceUseCase for handler tests ────────────────────

    struct MockUseCase;

    #[async_trait]
    impl InferenceUseCase for MockUseCase {
        async fn submit(
            &self,
            _prompt: &str,
            _model_name: &str,
            _backend_type: &str,
            _api_key_id: Option<Uuid>,
        ) -> Result<JobId> {
            Ok(JobId::new())
        }

        async fn process(&self, _job_id: &JobId) -> Result<()> {
            Ok(())
        }

        fn stream(
            &self,
            _job_id: &JobId,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
            let tokens = vec![
                Ok(StreamToken {
                    value: "Hello".to_string(),
                    is_final: false,
                    prompt_tokens: None,
                    completion_tokens: None,
                }),
                Ok(StreamToken {
                    value: "".to_string(),
                    is_final: true,
                    prompt_tokens: None,
                    completion_tokens: None,
                }),
            ];
            Box::pin(futures::stream::iter(tokens))
        }

        async fn get_status(&self, _job_id: &JobId) -> Result<JobStatus> {
            Ok(JobStatus::Running)
        }

        async fn cancel(&self, _job_id: &JobId) -> Result<()> {
            Ok(())
        }
    }

    struct MockApiKeyRepo;

    #[async_trait]
    impl ApiKeyRepository for MockApiKeyRepo {
        async fn create(&self, _key: &ApiKey) -> Result<()> {
            Ok(())
        }
        async fn get_by_hash(&self, _key_hash: &str) -> Result<Option<ApiKey>> {
            Ok(None)
        }
        async fn list_by_tenant(&self, _tenant_id: &str) -> Result<Vec<ApiKey>> {
            Ok(vec![])
        }
        async fn revoke(&self, _key_id: &Uuid) -> Result<()> {
            Ok(())
        }
    }

    struct MockBackendRegistry;

    #[async_trait]
    impl LlmBackendRegistry for MockBackendRegistry {
        async fn register(&self, _backend: &LlmBackend) -> Result<()> {
            Ok(())
        }
        async fn list_active(&self) -> Result<Vec<LlmBackend>> {
            Ok(vec![])
        }
        async fn list_all(&self) -> Result<Vec<LlmBackend>> {
            Ok(vec![])
        }
        async fn get(&self, _id: Uuid) -> Result<Option<LlmBackend>> {
            Ok(None)
        }
        async fn update_status(&self, _id: Uuid, _status: LlmBackendStatus) -> Result<()> {
            Ok(())
        }
        async fn deactivate(&self, _id: Uuid) -> Result<()> {
            Ok(())
        }
    }

    fn make_app() -> axum::Router {
        let fake_key = ApiKey {
            id: Uuid::now_v7(),
            key_hash: "testhash".to_string(),
            key_prefix: "iq_test".to_string(),
            tenant_id: "test-tenant".to_string(),
            name: "test-key".to_string(),
            is_active: true,
            rate_limit_rpm: 0,
            rate_limit_tpm: 0,
            expires_at: None,
            created_at: chrono::Utc::now(),
        };
        let pg_pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://test:test@localhost/test")
            .expect("lazy pool creation should not fail");
        let state = AppState {
            use_case: Arc::new(MockUseCase),
            api_key_repo: Arc::new(MockApiKeyRepo),
            backend_registry: Arc::new(MockBackendRegistry),
            valkey_pool: None,
            clickhouse_client: None,
            pg_pool,
        };
        // Inject a fake ApiKey extension so handlers that extract it work in tests.
        router::build_api_router()
            .layer(axum::Extension(fake_key))
            .with_state(state)
    }

    // ── submit_inference tests ─────────────────────────────────────

    #[tokio::test]
    async fn submit_valid_request_returns_200_with_job_id() {
        let app = make_app();
        let body = serde_json::json!({
            "prompt": "Hello world",
            "model": "llama3.2",
            "backend": "ollama"
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
        let resp: SubmitResponse = serde_json::from_slice(&body).unwrap();
        assert!(!resp.job_id.is_empty());
        // Verify it's a valid UUID
        assert!(Uuid::parse_str(&resp.job_id).is_ok());
    }

    #[tokio::test]
    async fn submit_with_default_backend() {
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
        let job_id = Uuid::now_v7();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/status", job_id))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: StatusResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.status, "running");
        assert_eq!(resp.job_id, job_id.to_string());
    }

    #[tokio::test]
    async fn get_status_invalid_job_id_returns_400() {
        let app = make_app();

        let request = Request::builder()
            .method("GET")
            .uri("/v1/inference/not-a-uuid/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ── cancel_inference tests ─────────────────────────────────────

    #[tokio::test]
    async fn cancel_valid_job_id_returns_200() {
        let app = make_app();
        let job_id = Uuid::now_v7();

        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/inference/{}", job_id))
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
            .uri("/v1/inference/not-a-uuid")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ── stream_inference tests ─────────────────────────────────────

    #[tokio::test]
    async fn stream_returns_sse_content_type() {
        let app = make_app();
        let job_id = Uuid::now_v7();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/stream", job_id))
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
        let job_id = Uuid::now_v7();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/stream", job_id))
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
        let job_id = Uuid::now_v7();

        let request = Request::builder()
            .method("GET")
            .uri(format!("/v1/inference/{}/stream", job_id))
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
