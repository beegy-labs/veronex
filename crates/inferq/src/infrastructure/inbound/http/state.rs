use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::application::ports::outbound::backend_model_selection::BackendModelSelectionRepository;
use crate::application::ports::outbound::gemini_model_repository::GeminiModelRepository;
use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;
use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::application::ports::outbound::ollama_sync_job_repository::OllamaSyncJobRepository;
use crate::infrastructure::outbound::hw_metrics::CpuSnapshot;

/// Shared application state passed to all HTTP handlers via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    pub use_case: Arc<dyn InferenceUseCase>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub backend_registry: Arc<dyn LlmBackendRegistry>,
    pub gpu_server_registry: Arc<dyn GpuServerRegistry>,
    pub gemini_policy_repo: Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo: Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo: Arc<dyn GeminiModelRepository>,
    pub model_selection_repo: Arc<dyn BackendModelSelectionRepository>,
    pub ollama_model_repo: Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo: Arc<dyn OllamaSyncJobRepository>,
    pub valkey_pool: Option<fred::clients::RedisPool>,
    pub clickhouse_client: Option<clickhouse::Client>,
    pub pg_pool: sqlx::PgPool,
    /// Per-server CPU counter snapshots for delta-based usage calculation.
    /// Keyed by GpuServer ID; updated on every metrics scrape.
    pub cpu_snapshot_cache: Arc<Mutex<HashMap<Uuid, CpuSnapshot>>>,
}
