use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use uuid::Uuid;

use crate::application::ports::outbound::message_store::VisionAnalysis;
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
    pub conversation_id: Option<uuid::Uuid>,
    /// Billing tier of the API key: `Some(KeyTier::Paid)` routes to the high-priority queue.
    /// `None` or `Some(KeyTier::Free)` uses the standard queue.
    pub key_tier: Option<KeyTier>,
    /// Base64 images for vision inference (/api/generate).
    pub images: Option<Vec<String>>,
    pub stop: Option<serde_json::Value>,
    pub seed: Option<u32>,
    pub response_format: Option<serde_json::Value>,
    pub frequency_penalty: Option<f64>,
    pub presence_penalty: Option<f64>,
    /// MCP agentic loop group ID — same UUID for all jobs in one run_loop() execution.
    /// None for non-MCP (single-turn) requests.
    pub mcp_loop_id: Option<uuid::Uuid>,
    /// Max tokens (output limit) already capped at the HTTP handler boundary.
    /// Passed through to `InferenceJob.max_tokens`.
    pub max_tokens: Option<u32>,
    /// Vision pre-processing result for image-bearing Tasks.
    /// Set by handlers that call `analyze_images_for_context()` before submission.
    pub vision_analysis: Option<VisionAnalysis>,
}

/// Snapshot of in-flight job counts — derived from the in-memory DashMap.
pub struct LiveCounts {
    pub pending: u32,
    pub running: u32,
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

    /// Return current in-memory pending and running counts.
    /// Used by the stats ticker to compute real-time queue metrics.
    fn get_live_counts(&self) -> LiveCounts;
}
