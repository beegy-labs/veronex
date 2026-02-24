use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;

use uuid::Uuid;

use crate::domain::enums::JobStatus;
use crate::domain::value_objects::{JobId, StreamToken};

/// Inbound port for inference operations.
///
/// Implemented by the application use-case and called by HTTP handlers.
#[async_trait]
pub trait InferenceUseCase: Send + Sync {
    /// Submit a new inference job and return its ID.
    ///
    /// `api_key_id` is forwarded to the use-case so it can record TPM usage
    /// against the correct key after the job completes.
    async fn submit(
        &self,
        prompt: &str,
        model_name: &str,
        backend_type: &str,
        api_key_id: Option<Uuid>,
    ) -> Result<JobId>;

    /// Process a job synchronously (used by the queue worker).
    async fn process(&self, job_id: &JobId) -> Result<()>;

    /// Stream tokens for a job via SSE.
    fn stream(&self, job_id: &JobId) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>;

    /// Get the current status of a job.
    async fn get_status(&self, job_id: &JobId) -> Result<JobStatus>;

    /// Cancel a pending or running job.
    async fn cancel(&self, job_id: &JobId) -> Result<()>;
}
