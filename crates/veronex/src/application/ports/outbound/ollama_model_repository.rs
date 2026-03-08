use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct OllamaModel {
    pub model_name: String,
    pub provider_id: Uuid,
    pub synced_at: DateTime<Utc>,
}

/// Model name with number of providers that carry it.
pub struct OllamaModelWithCount {
    pub model_name: String,
    pub provider_count: i64,
}

/// Provider info for use in "which servers have this model?" responses.
pub struct OllamaProviderForModel {
    pub provider_id: Uuid,
    pub name: String,
    pub url: String,
    pub status: String,
}

#[async_trait]
pub trait OllamaModelRepository: Send + Sync {
    /// Replace models for a single provider: DELETE all for provider + INSERT new list.
    async fn sync_provider_models(&self, provider_id: Uuid, model_names: &[String]) -> Result<()>;

    /// List all distinct model names across all providers, sorted.
    async fn list_all(&self) -> Result<Vec<String>>;

    /// List all distinct model names with per-model provider count, sorted.
    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>>;

    /// List provider IDs that have the given model synced.
    async fn providers_for_model(&self, model_name: &str) -> Result<Vec<Uuid>>;

    /// List provider info (id, name, url, status) for providers that have the given model.
    async fn providers_info_for_model(&self, model_name: &str) -> Result<Vec<OllamaProviderForModel>>;

    /// List all model names synced for a specific provider.
    async fn models_for_provider(&self, provider_id: Uuid) -> Result<Vec<String>>;
}
