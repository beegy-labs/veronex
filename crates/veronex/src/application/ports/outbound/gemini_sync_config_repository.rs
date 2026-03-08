use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait GeminiSyncConfigRepository: Send + Sync {
    /// Returns `None` if no key has been set yet.
    async fn get_api_key(&self) -> Result<Option<String>>;

    /// Upsert the admin API key.
    async fn set_api_key(&self, api_key: &str) -> Result<()>;
}
