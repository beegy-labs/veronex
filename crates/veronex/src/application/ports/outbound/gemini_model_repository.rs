use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub struct GeminiModel {
    pub model_name: String,
    pub synced_at: DateTime<Utc>,
}

#[async_trait]
pub trait GeminiModelRepository: Send + Sync {
    /// Replace the global model pool: DELETE all + INSERT the new list.
    async fn sync_models(&self, model_names: &[String]) -> Result<()>;

    /// List all global Gemini models ordered by name.
    async fn list(&self) -> Result<Vec<GeminiModel>>;
}
