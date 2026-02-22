use std::sync::Arc;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;

/// Shared application state passed to all HTTP handlers via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    pub use_case: Arc<dyn InferenceUseCase>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub valkey_pool: Option<fred::clients::RedisPool>,
    pub clickhouse_client: Option<clickhouse::Client>,
}
