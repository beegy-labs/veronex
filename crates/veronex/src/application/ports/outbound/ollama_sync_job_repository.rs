use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub struct OllamaSyncJob {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub total_providers: i32,
    pub done_providers: i32,
    pub results: serde_json::Value,
}

#[async_trait]
pub trait OllamaSyncJobRepository: Send + Sync {
    /// Create a new sync job record and return its ID.
    async fn create(&self, total_providers: i32) -> Result<Uuid>;

    /// Append one provider result and increment done_providers.
    async fn update_progress(&self, id: Uuid, result: serde_json::Value) -> Result<()>;

    /// Mark the job as completed.
    async fn complete(&self, id: Uuid) -> Result<()>;

    /// Return the most recently started sync job, or None if none exist.
    async fn get_latest(&self) -> Result<Option<OllamaSyncJob>>;
}
