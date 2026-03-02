use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::InferenceJob;
use crate::domain::enums::JobStatus;
use crate::domain::value_objects::JobId;

/// Outbound port for inference job persistence.
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn save(&self, job: &InferenceJob) -> Result<()>;
    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>>;
    async fn update_status(&self, job_id: &JobId, status: JobStatus) -> Result<()>;
    /// Atomically mark a job as Cancelled and record the exact cancellation timestamp.
    /// No-op if the job is already in a terminal state (Completed / Failed).
    async fn cancel_job(&self, job_id: &JobId, cancelled_at: DateTime<Utc>) -> Result<()>;
    /// Return all jobs currently in Pending or Running state, ordered by created_at ASC.
    /// Used on startup to recover jobs that were in-flight when the server last stopped.
    async fn list_pending(&self) -> Result<Vec<InferenceJob>>;
}
