use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct ProviderSelectedModel {
    pub provider_id: Uuid,
    pub model_name: String,
    pub is_enabled: bool,
    pub added_at: DateTime<Utc>,
}

#[async_trait]
pub trait ProviderModelSelectionRepository: Send + Sync {
    /// Upsert a list of models for a provider.
    ///
    /// New rows are inserted with `is_enabled = true`.
    /// Existing rows are untouched (their `is_enabled` state is preserved).
    async fn upsert_models(&self, provider_id: Uuid, models: &[String]) -> Result<()>;

    /// List all tracked models (enabled and disabled) for a provider.
    async fn list(&self, provider_id: Uuid) -> Result<Vec<ProviderSelectedModel>>;

    /// Enable or disable a single model for a provider.
    async fn set_enabled(&self, provider_id: Uuid, model_name: &str, enabled: bool) -> Result<()>;

    /// Return only the names of enabled models for a provider.
    /// Used by the router to filter routing candidates.
    async fn list_enabled(&self, provider_id: Uuid) -> Result<Vec<String>>;
}
