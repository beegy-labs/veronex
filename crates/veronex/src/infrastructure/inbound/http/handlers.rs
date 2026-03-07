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
use crate::domain::enums::{ApiFormat, JobSource, ProviderType};
use crate::domain::value_objects::JobId;

use super::constants::{GEMINI_TIER_FREE, PROVIDER_GEMINI, PROVIDER_OLLAMA, SSE_KEEP_ALIVE, SSE_MAX_CONNECTIONS, SSE_TIMEOUT};
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
pub(super) fn sanitize_sse_error(e: &anyhow::Error) -> String {
    let msg = e.to_string();
    let safe = if msg.contains("database") || msg.contains("sqlx") || msg.contains("postgres") {
        "internal processing error".to_string()
    } else if msg.contains("reqwest") || msg.contains("connect") || msg.contains("timeout") {
        "provider communication error".to_string()
    } else if msg.contains("capacity") || msg.contains("slot") {
        "service at capacity".to_string()
    } else if msg.contains("cancelled") || msg.contains("canceled") {
        "request cancelled".to_string()
    } else if msg.contains("token") && msg.contains("limit") {
        "token limit exceeded".to_string()
    } else {
        "inference failed".to_string()
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
            api_key_id: Some(api_key.id),
            account_id: None,
            source: JobSource::Api,
            api_format: ApiFormat::VeronexNative,
            messages: None,
            tools: None,
            request_path: Some("/v1/inference".to_string()),
            conversation_id: None,
            key_tier: Some(api_key.tier),
        })
        .await?;

    Ok(Json(SubmitResponse {
        job_id: job_id.to_string(),
    }))
}

/// GET /v1/inference/:job_id/stream - SSE token streaming.
pub async fn stream_inference(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let guard = match try_acquire_sse(&state.sse_connections) {
        Ok(g) => g,
        Err(resp) => return resp,
    };

    let sse_stream: SseStream = match Uuid::parse_str(&job_id) {
        Ok(uuid) => {
            let jid = JobId(uuid);
            let token_stream = state.use_case.stream(&jid);

            Box::pin(token_stream.map(move |result| {
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
            }))
        }
        Err(_) => Box::pin(futures::stream::once(async {
            Ok::<_, Infallible>(Event::default().event("error").data("invalid job_id format"))
        })),
    };

    sse_response(sse_stream)
}

/// GET /v1/inference/:job_id/status - Get job status.
pub async fn get_status(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, AppError> {
    let uuid = parse_uuid(&job_id)?;
    let jid = JobId(uuid);

    let status = state
        .use_case
        .get_status(&jid)
        .await?;

    let status_str = status.as_str().to_string();

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
) -> Response {
    let guard = match try_acquire_sse(&state.sse_connections) {
        Ok(g) => g,
        Err(resp) => return resp,
    };

    let jid = JobId(job_id);
    let chunk_id = format!("chatcmpl-{}", job_id);
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
                tracing::error!(job_id = %job_id, "SSE stream error: {e:?}");
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
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let uuid = parse_uuid(&job_id)?;
    let jid = JobId(uuid);

    state
        .use_case
        .cancel(&jid)
        .await?;

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
    use crate::application::ports::outbound::account_repository::AccountRepository;
    use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
    use crate::application::ports::outbound::lab_settings_repository::{LabSettings, LabSettingsRepository};
    use crate::application::ports::outbound::provider_model_selection::{ProviderModelSelectionRepository, ProviderSelectedModel};
    use crate::application::ports::outbound::gemini_model_repository::{GeminiModel, GeminiModelRepository};
    use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
    use crate::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;
    use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
    use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
    use crate::application::ports::outbound::ollama_model_repository::{OllamaProviderForModel, OllamaModelRepository, OllamaModelWithCount};
    use crate::application::ports::outbound::ollama_sync_job_repository::{OllamaSyncJob, OllamaSyncJobRepository};
    use crate::application::ports::outbound::session_repository::SessionRepository;
    use crate::domain::entities::{Account, ApiKey, GeminiRateLimitPolicy, GpuServer, LlmProvider, Session};
    use crate::domain::enums::{JobStatus, KeyTier, KeyType, LlmProviderStatus};
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
        async fn submit(&self, _req: SubmitJobRequest) -> Result<JobId> {
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
                    tool_calls: None,
                }),
                Ok(StreamToken {
                    value: "".to_string(),
                    is_final: true,
                    prompt_tokens: None,
                    completion_tokens: None,
                    cached_tokens: None,
                    tool_calls: None,
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
        async fn get_by_id(&self, _key_id: &Uuid) -> Result<Option<ApiKey>> {
            Ok(None)
        }
        async fn get_by_hash(&self, _key_hash: &str) -> Result<Option<ApiKey>> {
            Ok(None)
        }
        async fn list_by_tenant(&self, _tenant_id: &str) -> Result<Vec<ApiKey>> {
            Ok(vec![])
        }
        async fn list_all(&self) -> Result<Vec<ApiKey>> {
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
        async fn set_tier(&self, _key_id: &Uuid, _tier: &KeyTier) -> Result<()> {
            Ok(())
        }
        async fn update_fields(&self, _key_id: &Uuid, _is_active: Option<bool>, _tier: Option<&KeyTier>) -> Result<()> {
            Ok(())
        }
        async fn soft_delete_by_tenant(&self, _tenant_id: &str) -> Result<u64> {
            Ok(0)
        }
    }

    struct MockProviderRegistry;

    #[async_trait]
    impl LlmProviderRegistry for MockProviderRegistry {
        async fn register(&self, _provider: &LlmProvider) -> Result<()> { Ok(()) }
        async fn list_active(&self) -> Result<Vec<LlmProvider>> { Ok(vec![]) }
        async fn list_all(&self) -> Result<Vec<LlmProvider>> { Ok(vec![]) }
        async fn get(&self, _id: Uuid) -> Result<Option<LlmProvider>> { Ok(None) }
        async fn update_status(&self, _id: Uuid, _status: LlmProviderStatus) -> Result<()> { Ok(()) }
        async fn deactivate(&self, _id: Uuid) -> Result<()> { Ok(()) }
        async fn update(&self, _provider: &LlmProvider) -> Result<()> { Ok(()) }
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
    impl ProviderModelSelectionRepository for MockModelSelectionRepo {
        async fn upsert_models(&self, _provider_id: Uuid, _models: &[String]) -> Result<()> { Ok(()) }
        async fn list(&self, _provider_id: Uuid) -> Result<Vec<ProviderSelectedModel>> { Ok(vec![]) }
        async fn set_enabled(&self, _provider_id: Uuid, _model_name: &str, _enabled: bool) -> Result<()> { Ok(()) }
        async fn list_enabled(&self, _provider_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
    }

    struct MockOllamaModelRepo;

    #[async_trait]
    impl OllamaModelRepository for MockOllamaModelRepo {
        async fn sync_provider_models(&self, _provider_id: Uuid, _model_names: &[String]) -> Result<()> { Ok(()) }
        async fn list_all(&self) -> Result<Vec<String>> { Ok(vec![]) }
        async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>> { Ok(vec![]) }
        async fn providers_for_model(&self, _model_name: &str) -> Result<Vec<Uuid>> { Ok(vec![]) }
        async fn providers_info_for_model(&self, _model_name: &str) -> Result<Vec<OllamaProviderForModel>> { Ok(vec![]) }
        async fn models_for_provider(&self, _provider_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
    }

    struct MockOllamaSyncJobRepo;

    #[async_trait]
    impl OllamaSyncJobRepository for MockOllamaSyncJobRepo {
        async fn create(&self, _total_providers: i32) -> Result<Uuid> { Ok(Uuid::now_v7()) }
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
        async fn soft_delete_cascade(&self, _account_id: &Uuid, _tenant_id: &str) -> Result<u64> { Ok(0) }
        async fn set_active(&self, _id: &Uuid, _is_active: bool) -> Result<()> { Ok(()) }
        async fn update_last_login(&self, _id: &Uuid) -> Result<()> { Ok(()) }
        async fn set_password_hash(&self, _id: &Uuid, _hash: &str) -> Result<()> { Ok(()) }
    }

    struct MockCapacityRepo;

    #[async_trait]
    impl crate::application::ports::outbound::model_capacity_repository::ModelCapacityRepository for MockCapacityRepo {
        async fn upsert(&self, _: &crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry) -> Result<()> { Ok(()) }
        async fn get(&self, _: uuid::Uuid, _: &str) -> Result<Option<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(None) }
        async fn list_all(&self) -> Result<Vec<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(vec![]) }
        async fn list_by_provider(&self, _: uuid::Uuid) -> Result<Vec<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(vec![]) }
        async fn compute_throughput_stats(&self, _: uuid::Uuid, _: &str, _: u32) -> Result<Option<crate::application::ports::outbound::model_capacity_repository::ThroughputStats>> { Ok(None) }
    }

    struct MockCapacitySettingsRepo;

    #[async_trait]
    impl crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository for MockCapacitySettingsRepo {
        async fn get(&self) -> Result<crate::application::ports::outbound::capacity_settings_repository::CapacitySettings> {
            Ok(crate::application::ports::outbound::capacity_settings_repository::CapacitySettings::default())
        }
        async fn update_settings(&self, _: Option<&str>, _: Option<bool>, _: Option<i32>, _: Option<i32>, _: Option<i32>) -> Result<crate::application::ports::outbound::capacity_settings_repository::CapacitySettings> {
            Ok(crate::application::ports::outbound::capacity_settings_repository::CapacitySettings::default())
        }
        async fn record_run(&self, _: &str) -> Result<()> { Ok(()) }
    }

    struct MockLabSettingsRepo;

    #[async_trait]
    impl LabSettingsRepository for MockLabSettingsRepo {
        async fn get(&self) -> Result<LabSettings> {
            Ok(LabSettings { gemini_function_calling: false, updated_at: chrono::Utc::now() })
        }
        async fn update(&self, _gemini_function_calling: Option<bool>) -> Result<LabSettings> {
            Ok(LabSettings { gemini_function_calling: false, updated_at: chrono::Utc::now() })
        }
    }

    struct MockSessionRepo;

    #[async_trait]
    impl SessionRepository for MockSessionRepo {
        async fn create(&self, _session: &Session) -> Result<()> { Ok(()) }
        async fn list_active(&self, _account_id: &Uuid) -> Result<Vec<Session>> { Ok(vec![]) }
        async fn get_by_refresh_hash(&self, _hash: &str) -> Result<Option<Session>> { Ok(None) }
        async fn revoke(&self, _session_id: &Uuid) -> Result<()> { Ok(()) }
        async fn get_by_id(&self, _session_id: &Uuid) -> Result<Option<Session>> { Ok(None) }
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
            key_type: KeyType::Standard,
            tier: KeyTier::Paid,
        };
        let pg_pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://test:test@localhost/test")
            .expect("lazy pool creation should not fail");
        let state = AppState {
            http_client: reqwest::Client::new(),
            use_case: Arc::new(MockUseCase),
            api_key_repo: Arc::new(MockApiKeyRepo),
            account_repo: Arc::new(MockAccountRepo),
            audit_port: None,
            jwt_secret: "test-secret".to_string(),
            provider_registry: Arc::new(MockProviderRegistry),
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
            vram_pool: Arc::new(crate::infrastructure::outbound::capacity::vram_pool::VramPool::new()) as Arc<dyn crate::application::ports::outbound::concurrency_port::VramPoolPort>,
            thermal: Arc::new(crate::infrastructure::outbound::capacity::thermal::ThermalThrottleMap::new(60)),
            capacity_repo: Arc::new(MockCapacityRepo),
            capacity_settings_repo: Arc::new(MockCapacitySettingsRepo),
            sync_trigger: Arc::new(tokio::sync::Notify::new()),
            analyzer_url: String::new(),
            job_event_tx: Arc::new(tokio::sync::broadcast::channel(1).0),
            circuit_breaker: Arc::new(crate::infrastructure::outbound::circuit_breaker::CircuitBreakerMap::new()),
            message_store: None,
            session_grouping_lock: Arc::new(tokio::sync::Semaphore::new(1)),
            sync_lock: Arc::new(tokio::sync::Semaphore::new(1)),
            lab_settings_repo: Arc::new(MockLabSettingsRepo),
            sse_connections: Arc::new(AtomicU32::new(0)),
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
        let resp: SubmitResponse = serde_json::from_slice(&body).unwrap();
        assert!(!resp.job_id.is_empty());
        // Verify it's a valid UUID
        assert!(Uuid::parse_str(&resp.job_id).is_ok());
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
