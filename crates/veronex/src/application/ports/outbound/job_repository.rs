use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::entities::InferenceJob;
use crate::domain::enums::JobStatus;
use crate::domain::value_objects::JobId;

/// Outbound port for inference job persistence.
///
/// # Write discipline (simplified S3 design)
///
/// `save()` is the **only INSERT** — initial Pending row, metadata only.
/// All large content (prompt, messages, result, tool_calls) lives in S3.
///
/// `finalize()` is the **single terminal UPDATE** — written once at stream end
/// with all execution metrics (latency, tokens, provider). No intermediate
/// state transitions are written to Postgres.
///
/// `cancel_job()` and `fail_with_reason()` handle the two early-exit paths
/// (explicit cancel and queue-full / stream error).
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Insert the initial job row (Pending state).
    /// Only `prompt_preview` (≤200 chars) is stored — full content goes to S3.
    async fn save(&self, job: &InferenceJob) -> Result<()>;

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>>;

    async fn update_status(&self, job_id: &JobId, status: JobStatus) -> Result<()>;

    /// Atomically mark a job as Cancelled and record the exact cancellation timestamp.
    /// No-op if the job is already in a terminal state (Completed / Failed).
    async fn cancel_job(&self, job_id: &JobId, cancelled_at: DateTime<Utc>) -> Result<()>;

    /// Atomically mark a job as Failed with a machine-readable failure_reason.
    /// Used for queue-full rejections and provider stream errors.
    async fn fail_with_reason(
        &self,
        job_id: &JobId,
        reason: &str,
        error_msg: Option<&str>,
    ) -> Result<()>;

    /// Single terminal UPDATE for a completed job.
    ///
    /// Called once at stream end with all execution metrics.
    /// Replaces the former mark_running + mark_completed two-step.
    #[allow(clippy::too_many_arguments)]
    async fn finalize(
        &self,
        job_id: &JobId,
        started_at: Option<DateTime<Utc>>,
        completed_at: DateTime<Utc>,
        provider_id: Option<Uuid>,
        queue_time_ms: Option<i32>,
        latency_ms: i32,
        ttft_ms: Option<i32>,
        prompt_tokens: Option<i32>,
        completion_tokens: Option<i32>,
        cached_tokens: Option<i32>,
        has_tool_calls: bool,
        result_preview: Option<&str>,
    ) -> Result<()>;

    /// Persist image object-storage keys after async upload completes.
    async fn update_image_keys(&self, job_id: &JobId, image_keys: Vec<String>) -> Result<()>;

    /// Increment conversation counters after a job completes.
    async fn update_conversation_counters(
        &self,
        conversation_id: &Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
        model_name: &str,
    ) -> Result<()>;

    /// Return all jobs currently in Pending or Running state, ordered by created_at ASC.
    /// Used on startup to recover jobs that were in-flight when the server last stopped.
    async fn list_pending(&self) -> Result<Vec<InferenceJob>>;

    /// Returns seconds elapsed since the most recent non-analyzer
    /// (`source IN ('api','test')`) job's `created_at`. `None` if no such row
    /// exists. Used by the capacity analyzer's demand gate to skip ticks when
    /// the cluster has been idle from real user traffic.
    /// SDD: `.specs/veronex/history/inference-mcp-per-round-persist.md` §6.
    async fn seconds_since_last_user_job(&self) -> Result<Option<i64>>;
}
