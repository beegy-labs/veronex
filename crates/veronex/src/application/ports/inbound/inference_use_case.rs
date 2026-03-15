use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use uuid::Uuid;

use crate::domain::enums::{ApiFormat, JobSource, JobStatus, KeyTier, ProviderType};
use crate::domain::errors::DomainError;
use crate::domain::value_objects::{JobId, StreamToken};

type Result<T> = std::result::Result<T, DomainError>;

/// Parameters for submitting a new inference job.
pub struct SubmitJobRequest {
    pub prompt: String,
    pub model_name: String,
    pub provider_type: ProviderType,
    /// Gemini tier routing preference: `Some("free")` = free-tier only, `None` = auto.
    /// Parsed from "gemini-free" at the HTTP handler boundary.
    pub gemini_tier: Option<String>,
    pub api_key_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub source: JobSource,
    pub api_format: ApiFormat,
    pub messages: Option<serde_json::Value>,
    pub tools: Option<serde_json::Value>,
    pub request_path: Option<String>,
    pub conversation_id: Option<String>,
    /// Billing tier of the API key: `Some(KeyTier::Paid)` routes to the high-priority queue.
    /// `None` or `Some(KeyTier::Free)` uses the standard queue.
    pub key_tier: Option<KeyTier>,
    /// Base64 images for vision inference (/api/generate).
    pub images: Option<Vec<String>>,
}

/// Inbound port for inference operations.
///
/// Implemented by the application use-case and called by HTTP handlers.
#[async_trait]
pub trait InferenceUseCase: Send + Sync {
    /// Submit a new inference job and return its ID.
    async fn submit(&self, req: SubmitJobRequest) -> Result<JobId>;

    /// Process a job synchronously (used by the queue worker).
    async fn process(&self, job_id: &JobId) -> Result<()>;

    /// Stream tokens for a job via SSE.
    fn stream(&self, job_id: &JobId) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>;

    /// Get the current status of a job.
    async fn get_status(&self, job_id: &JobId) -> Result<JobStatus>;

    /// Cancel a pending or running job.
    async fn cancel(&self, job_id: &JobId) -> Result<()>;
}
