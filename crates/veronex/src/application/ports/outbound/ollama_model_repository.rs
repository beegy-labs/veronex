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
    pub is_enabled: bool,
}

/// Paginated model list result.
pub struct ModelPage {
    pub items: Vec<OllamaModelWithCount>,
    pub total: i64,
}

/// Paginated provider-for-model result.
pub struct ProviderPage {
    pub items: Vec<OllamaProviderForModel>,
    pub total: i64,
}

#[async_trait]
pub trait OllamaModelRepository: Send + Sync {
    /// Replace models for a single provider: DELETE all for provider + INSERT new list.
    async fn sync_provider_models(&self, provider_id: Uuid, model_names: &[String]) -> Result<()>;

    /// List all distinct model names across all providers, sorted.
    async fn list_all(&self) -> Result<Vec<String>>;

    /// List all distinct model names with per-model provider count, sorted (no pagination — internal use only).
    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>>;

    /// Paginated + searchable model list for the API.
    async fn list_with_counts_page(&self, search: &str, limit: i64, offset: i64) -> Result<ModelPage>;

    /// List provider IDs that have the given model synced.
    async fn providers_for_model(&self, model_name: &str) -> Result<Vec<Uuid>>;

    /// Paginated + searchable provider list for a given model, including per-provider is_enabled.
    async fn providers_info_for_model_page(
        &self,
        model_name: &str,
        search: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ProviderPage>;

    /// List all model names synced for a specific provider.
    async fn models_for_provider(&self, provider_id: Uuid) -> Result<Vec<String>>;
}
