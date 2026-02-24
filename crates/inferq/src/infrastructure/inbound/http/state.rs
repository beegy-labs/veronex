use std::sync::Arc;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;

/// Shared application state passed to all HTTP handlers via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    pub use_case: Arc<dyn InferenceUseCase>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub backend_registry: Arc<dyn LlmBackendRegistry>,
    pub valkey_pool: Option<fred::clients::RedisPool>,
    pub clickhouse_client: Option<clickhouse::Client>,
    pub pg_pool: sqlx::PgPool,
}
