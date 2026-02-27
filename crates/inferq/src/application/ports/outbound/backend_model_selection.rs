use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct BackendSelectedModel {
    pub backend_id: Uuid,
    pub model_name: String,
    pub is_enabled: bool,
    pub added_at: DateTime<Utc>,
}

#[async_trait]
pub trait BackendModelSelectionRepository: Send + Sync {
    /// Upsert a list of models for a backend.
    ///
    /// New rows are inserted with `is_enabled = true`.
    /// Existing rows are untouched (their `is_enabled` state is preserved).
    async fn upsert_models(&self, backend_id: Uuid, models: &[String]) -> Result<()>;

    /// List all tracked models (enabled and disabled) for a backend.
    async fn list(&self, backend_id: Uuid) -> Result<Vec<BackendSelectedModel>>;

    /// Enable or disable a single model for a backend.
    async fn set_enabled(&self, backend_id: Uuid, model_name: &str, enabled: bool) -> Result<()>;

    /// Return only the names of enabled models for a backend.
    /// Used by the router to filter routing candidates.
    async fn list_enabled(&self, backend_id: Uuid) -> Result<Vec<String>>;
}
