use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::LlmProvider;
use crate::domain::enums::LlmProviderStatus;

/// Outbound port for registering and querying LLM providers.
#[async_trait]
pub trait LlmProviderRegistry: Send + Sync {
    /// Persist a new provider record (INSERT).
    async fn register(&self, provider: &LlmProvider) -> Result<()>;

    /// Return all active + online providers (used for routing).
    async fn list_active(&self) -> Result<Vec<LlmProvider>>;

    /// Return all providers regardless of status (used for management UI).
    async fn list_all(&self) -> Result<Vec<LlmProvider>>;

    /// Look up a single provider by ID.
    async fn get(&self, id: Uuid) -> Result<Option<LlmProvider>>;

    /// Update the online/offline/degraded health status.
    async fn update_status(&self, id: Uuid, status: LlmProviderStatus) -> Result<()>;

    /// Soft-delete: mark the provider as inactive so it is excluded from routing.
    async fn deactivate(&self, id: Uuid) -> Result<()>;

    /// Update mutable fields (name, url, api_key_encrypted, total_vram_mb,
    /// gpu_index, server_id).  Status and registered_at are unchanged.
    async fn update(&self, provider: &LlmProvider) -> Result<()>;
}
