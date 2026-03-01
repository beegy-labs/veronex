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

use crate::domain::enums::{ApiFormat, JobSource};
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
        .submit(&req.prompt, &req.model, &req.backend, Some(api_key.id), None, JobSource::Api, ApiFormat::VeronexNative, None, Some("/v1/inference".to_string()))
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

/// GET /v1/jobs/:job_id/stream — OpenAI-format SSE replay for test reconnect.
///
/// Streams a job's tokens in the same OpenAI chunk format as `/v1/chat/completions`.
/// Completed jobs are replayed from the DB; in-progress jobs stream live tokens.
pub async fn stream_job_openai(
    Path(job_id): Path<Uuid>,
    State(state): State<AppState>,
    axum::extract::Extension(_api_key): axum::extract::Extension<crate::domain::entities::ApiKey>,
) -> impl IntoResponse {
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
        choices: Vec<ChunkChoice>,
    }

    let jid = JobId(job_id);
    let chunk_id = format!("chatcmpl-{}", job_id);
    let created = chrono::Utc::now().timestamp();
    let token_stream = state.use_case.stream(&jid);

    let content_stream = token_stream.map(move |result| -> Result<Event, std::convert::Infallible> {
        match result {
            Ok(token) if token.is_final => {
                let stop_chunk = CompletionChunk {
                    id: chunk_id.clone(),
                    object: "chat.completion.chunk",
                    created,
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

    let done_stream = futures::stream::once(async {
        Ok::<_, std::convert::Infallible>(Event::default().data("[DONE]"))
    });
    let sse_stream: SseStream = Box::pin(content_stream.chain(done_stream));

    (
        [("X-Accel-Buffering", "no")],
        axum::response::sse::Sse::new(sse_stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
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
    use crate::application::ports::outbound::account_repository::AccountRepository;
    use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
    use crate::application::ports::outbound::backend_model_selection::{BackendModelSelectionRepository, BackendSelectedModel};
    use crate::application::ports::outbound::gemini_model_repository::{GeminiModel, GeminiModelRepository};
    use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
    use crate::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;
    use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
    use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
    use crate::application::ports::outbound::ollama_model_repository::{OllamaBackendForModel, OllamaModelRepository, OllamaModelWithCount};
    use crate::application::ports::outbound::ollama_sync_job_repository::{OllamaSyncJob, OllamaSyncJobRepository};
    use crate::application::ports::outbound::session_repository::SessionRepository;
    use crate::domain::entities::{Account, ApiKey, GeminiRateLimitPolicy, GpuServer, LlmBackend, Session};
    use crate::domain::enums::{JobSource, JobStatus, LlmBackendStatus};
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
            _account_id: Option<Uuid>,
            _source: JobSource,
            _api_format: ApiFormat,
            _messages: Option<serde_json::Value>,
            _request_path: Option<String>,
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
                    cached_tokens: None,
                }),
                Ok(StreamToken {
                    value: "".to_string(),
                    is_final: true,
                    prompt_tokens: None,
                    completion_tokens: None,
                    cached_tokens: None,
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
        async fn set_active(&self, _key_id: &Uuid, _active: bool) -> Result<()> {
            Ok(())
        }
        async fn soft_delete(&self, _key_id: &Uuid) -> Result<()> {
            Ok(())
        }
    }

    struct MockBackendRegistry;

    #[async_trait]
    impl LlmBackendRegistry for MockBackendRegistry {
        async fn register(&self, _backend: &LlmBackend) -> Result<()> { Ok(()) }
        async fn list_active(&self) -> Result<Vec<LlmBackend>> { Ok(vec![]) }
        async fn list_all(&self) -> Result<Vec<LlmBackend>> { Ok(vec![]) }
        async fn get(&self, _id: Uuid) -> Result<Option<LlmBackend>> { Ok(None) }
        async fn update_status(&self, _id: Uuid, _status: LlmBackendStatus) -> Result<()> { Ok(()) }
        async fn deactivate(&self, _id: Uuid) -> Result<()> { Ok(()) }
        async fn update(&self, _backend: &LlmBackend) -> Result<()> { Ok(()) }
    }

    struct MockGpuServerRegistry;

    #[async_trait]
    impl GpuServerRegistry for MockGpuServerRegistry {
        async fn register(&self, _server: GpuServer) -> Result<()> { Ok(()) }
        async fn list_all(&self) -> Result<Vec<GpuServer>> { Ok(vec![]) }
        async fn get(&self, _id: Uuid) -> Result<Option<GpuServer>> { Ok(None) }
        async fn update(&self, _server: &GpuServer) -> Result<()> { Ok(()) }
        async fn delete(&self, _id: Uuid) -> Result<()> { Ok(()) }
    }

    struct MockGeminiPolicyRepo;

    #[async_trait]
    impl GeminiPolicyRepository for MockGeminiPolicyRepo {
        async fn list_all(&self) -> Result<Vec<GeminiRateLimitPolicy>> { Ok(vec![]) }
        async fn get_for_model(&self, _model_name: &str) -> Result<Option<GeminiRateLimitPolicy>> { Ok(None) }
        async fn upsert(&self, _policy: &GeminiRateLimitPolicy) -> Result<()> { Ok(()) }
    }

    struct MockGeminiSyncConfigRepo;

    #[async_trait]
    impl GeminiSyncConfigRepository for MockGeminiSyncConfigRepo {
        async fn get_api_key(&self) -> Result<Option<String>> { Ok(None) }
        async fn set_api_key(&self, _api_key: &str) -> Result<()> { Ok(()) }
    }

    struct MockGeminiModelRepo;

    #[async_trait]
    impl GeminiModelRepository for MockGeminiModelRepo {
        async fn sync_models(&self, _model_names: &[String]) -> Result<()> { Ok(()) }
        async fn list(&self) -> Result<Vec<GeminiModel>> { Ok(vec![]) }
    }

    struct MockModelSelectionRepo;

    #[async_trait]
    impl BackendModelSelectionRepository for MockModelSelectionRepo {
        async fn upsert_models(&self, _backend_id: Uuid, _models: &[String]) -> Result<()> { Ok(()) }
        async fn list(&self, _backend_id: Uuid) -> Result<Vec<BackendSelectedModel>> { Ok(vec![]) }
        async fn set_enabled(&self, _backend_id: Uuid, _model_name: &str, _enabled: bool) -> Result<()> { Ok(()) }
        async fn list_enabled(&self, _backend_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
    }

    struct MockOllamaModelRepo;

    #[async_trait]
    impl OllamaModelRepository for MockOllamaModelRepo {
        async fn sync_backend_models(&self, _backend_id: Uuid, _model_names: &[String]) -> Result<()> { Ok(()) }
        async fn list_all(&self) -> Result<Vec<String>> { Ok(vec![]) }
        async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>> { Ok(vec![]) }
        async fn backends_for_model(&self, _model_name: &str) -> Result<Vec<Uuid>> { Ok(vec![]) }
        async fn backends_info_for_model(&self, _model_name: &str) -> Result<Vec<OllamaBackendForModel>> { Ok(vec![]) }
        async fn models_for_backend(&self, _backend_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
    }

    struct MockOllamaSyncJobRepo;

    #[async_trait]
    impl OllamaSyncJobRepository for MockOllamaSyncJobRepo {
        async fn create(&self, _total_backends: i32) -> Result<Uuid> { Ok(Uuid::now_v7()) }
        async fn update_progress(&self, _id: Uuid, _result: serde_json::Value) -> Result<()> { Ok(()) }
        async fn complete(&self, _id: Uuid) -> Result<()> { Ok(()) }
        async fn get_latest(&self) -> Result<Option<OllamaSyncJob>> { Ok(None) }
    }

    struct MockAccountRepo;

    #[async_trait]
    impl AccountRepository for MockAccountRepo {
        async fn create(&self, _account: &Account) -> Result<()> { Ok(()) }
        async fn get_by_id(&self, _id: &Uuid) -> Result<Option<Account>> { Ok(None) }
        async fn get_by_username(&self, _username: &str) -> Result<Option<Account>> { Ok(None) }
        async fn list_all(&self) -> Result<Vec<Account>> { Ok(vec![]) }
        async fn update(&self, _account: &Account) -> Result<()> { Ok(()) }
        async fn soft_delete(&self, _id: &Uuid) -> Result<()> { Ok(()) }
        async fn set_active(&self, _id: &Uuid, _is_active: bool) -> Result<()> { Ok(()) }
        async fn update_last_login(&self, _id: &Uuid) -> Result<()> { Ok(()) }
        async fn set_password_hash(&self, _id: &Uuid, _hash: &str) -> Result<()> { Ok(()) }
    }

    struct MockCapacityRepo;

    #[async_trait]
    impl crate::application::ports::outbound::model_capacity_repository::ModelCapacityRepository for MockCapacityRepo {
        async fn upsert(&self, _: &crate::application::ports::outbound::model_capacity_repository::ModelCapacityEntry) -> Result<()> { Ok(()) }
        async fn get(&self, _: uuid::Uuid, _: &str) -> Result<Option<crate::application::ports::outbound::model_capacity_repository::ModelCapacityEntry>> { Ok(None) }
        async fn list_all(&self) -> Result<Vec<crate::application::ports::outbound::model_capacity_repository::ModelCapacityEntry>> { Ok(vec![]) }
        async fn compute_throughput_stats(&self, _: uuid::Uuid, _: &str, _: u32) -> Result<Option<crate::application::ports::outbound::model_capacity_repository::ThroughputStats>> { Ok(None) }
    }

    struct MockCapacitySettingsRepo;

    #[async_trait]
    impl crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository for MockCapacitySettingsRepo {
        async fn get(&self) -> Result<crate::application::ports::outbound::capacity_settings_repository::CapacitySettings> {
            Ok(crate::application::ports::outbound::capacity_settings_repository::CapacitySettings::default())
        }
        async fn update_settings(&self, _: Option<&str>, _: Option<bool>, _: Option<i32>) -> Result<crate::application::ports::outbound::capacity_settings_repository::CapacitySettings> {
            Ok(crate::application::ports::outbound::capacity_settings_repository::CapacitySettings::default())
        }
        async fn record_run(&self, _: &str) -> Result<()> { Ok(()) }
    }

    struct MockSessionRepo;

    #[async_trait]
    impl SessionRepository for MockSessionRepo {
        async fn create(&self, _session: &Session) -> Result<()> { Ok(()) }
        async fn list_active(&self, _account_id: &Uuid) -> Result<Vec<Session>> { Ok(vec![]) }
        async fn get_by_refresh_hash(&self, _hash: &str) -> Result<Option<Session>> { Ok(None) }
        async fn revoke(&self, _session_id: &Uuid) -> Result<()> { Ok(()) }
        async fn revoke_all_for_account(&self, _account_id: &Uuid) -> Result<()> { Ok(()) }
        async fn update_last_used(&self, _jti: &Uuid) -> Result<()> { Ok(()) }
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
            deleted_at: None,
            created_at: chrono::Utc::now(),
            key_type: "standard".to_string(),
            tier: "paid".to_string(),
        };
        let pg_pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://test:test@localhost/test")
            .expect("lazy pool creation should not fail");
        let state = AppState {
            use_case: Arc::new(MockUseCase),
            api_key_repo: Arc::new(MockApiKeyRepo),
            account_repo: Arc::new(MockAccountRepo),
            audit_port: None,
            jwt_secret: "test-secret".to_string(),
            backend_registry: Arc::new(MockBackendRegistry),
            gpu_server_registry: Arc::new(MockGpuServerRegistry),
            gemini_policy_repo: Arc::new(MockGeminiPolicyRepo),
            gemini_sync_config_repo: Arc::new(MockGeminiSyncConfigRepo),
            gemini_model_repo: Arc::new(MockGeminiModelRepo),
            model_selection_repo: Arc::new(MockModelSelectionRepo),
            ollama_model_repo: Arc::new(MockOllamaModelRepo),
            ollama_sync_job_repo: Arc::new(MockOllamaSyncJobRepo),
            valkey_pool: None,
            analytics_repo: None,
            session_repo: Arc::new(MockSessionRepo),
            pg_pool,
            cpu_snapshot_cache: Arc::new(dashmap::DashMap::new()),
            slot_map: Arc::new(crate::infrastructure::outbound::capacity::slot_map::ConcurrencySlotMap::new()),
            thermal: Arc::new(crate::infrastructure::outbound::capacity::thermal::ThermalThrottleMap::new(60)),
            capacity_repo: Arc::new(MockCapacityRepo),
            capacity_settings_repo: Arc::new(MockCapacitySettingsRepo),
            capacity_manual_trigger: Arc::new(tokio::sync::Notify::new()),
            analyzer_url: String::new(),
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
