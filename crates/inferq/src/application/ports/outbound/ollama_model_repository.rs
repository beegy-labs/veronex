use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct OllamaModel {
    pub model_name: String,
    pub backend_id: Uuid,
    pub synced_at: DateTime<Utc>,
}

/// Model name with number of backends that carry it.
pub struct OllamaModelWithCount {
    pub model_name: String,
    pub backend_count: i64,
}

/// Backend info for use in "which servers have this model?" responses.
pub struct OllamaBackendForModel {
    pub backend_id: Uuid,
    pub name: String,
    pub url: String,
    pub status: String,
}

#[async_trait]
pub trait OllamaModelRepository: Send + Sync {
    /// Replace models for a single backend: DELETE all for backend + INSERT new list.
    async fn sync_backend_models(&self, backend_id: Uuid, model_names: &[String]) -> Result<()>;

    /// List all distinct model names across all backends, sorted.
    async fn list_all(&self) -> Result<Vec<String>>;

    /// List all distinct model names with per-model backend count, sorted.
    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>>;

    /// List backend IDs that have the given model synced.
    async fn backends_for_model(&self, model_name: &str) -> Result<Vec<Uuid>>;

    /// List backend info (id, name, url, status) for backends that have the given model.
    async fn backends_info_for_model(&self, model_name: &str) -> Result<Vec<OllamaBackendForModel>>;

    /// List all model names synced for a specific backend.
    async fn models_for_backend(&self, backend_id: Uuid) -> Result<Vec<String>>;
}
