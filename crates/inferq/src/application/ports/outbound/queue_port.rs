use anyhow::Result;
use async_trait::async_trait;

use crate::domain::value_objects::JobId;

/// Outbound port for the inference job queue.
#[async_trait]
pub trait QueuePort: Send + Sync {
    async fn enqueue(&self, job_id: &JobId) -> Result<()>;
    async fn dequeue(&self) -> Result<Option<JobId>>;
}
