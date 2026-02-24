use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::LlmBackend;
use crate::domain::enums::LlmBackendStatus;

/// Outbound port for registering and querying LLM backends.
#[async_trait]
pub trait LlmBackendRegistry: Send + Sync {
    /// Persist a new backend record (INSERT).
    async fn register(&self, backend: &LlmBackend) -> Result<()>;

    /// Return all active + online backends (used for routing).
    async fn list_active(&self) -> Result<Vec<LlmBackend>>;

    /// Return all backends regardless of status (used for management UI).
    async fn list_all(&self) -> Result<Vec<LlmBackend>>;

    /// Look up a single backend by ID.
    async fn get(&self, id: Uuid) -> Result<Option<LlmBackend>>;

    /// Update the online/offline/degraded health status.
    async fn update_status(&self, id: Uuid, status: LlmBackendStatus) -> Result<()>;

    /// Soft-delete: mark the backend as inactive so it is excluded from routing.
    async fn deactivate(&self, id: Uuid) -> Result<()>;
}
