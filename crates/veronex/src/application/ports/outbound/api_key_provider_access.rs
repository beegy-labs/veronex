use async_trait::async_trait;
use uuid::Uuid;

/// API key → provider access control.
/// No rows = all providers allowed (default allow-all).
/// When rows exist, only providers with is_allowed = true are routable.
#[async_trait]
pub trait ApiKeyProviderAccessRepository: Send + Sync {
    /// List provider IDs allowed for this key. Empty vec = no restrictions.
    async fn list_allowed(&self, api_key_id: Uuid) -> anyhow::Result<Vec<Uuid>>;

    /// Set allow/deny for a specific key+provider pair (upsert).
    async fn set_access(&self, api_key_id: Uuid, provider_id: Uuid, allowed: bool) -> anyhow::Result<()>;

    /// List all access rules for a key.
    async fn list(&self, api_key_id: Uuid) -> anyhow::Result<Vec<(Uuid, bool)>>;

    /// Check if a key has any access restrictions.
    async fn has_restrictions(&self, api_key_id: Uuid) -> anyhow::Result<bool>;
}
