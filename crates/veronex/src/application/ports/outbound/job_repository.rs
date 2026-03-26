use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::entities::InferenceJob;
use crate::domain::enums::JobStatus;
use crate::domain::value_objects::JobId;

/// Outbound port for inference job persistence.
///
/// # Write discipline
///
/// `save()` is the **single insert point** — it performs the initial `INSERT` for a new
/// job and is the only path that creates a row.  All subsequent state transitions use
/// targeted `UPDATE` methods (`mark_running`, `mark_completed`, `fail_with_reason`,
/// `cancel_job`, `update_image_keys`) that touch only the columns they own.
///
/// This pattern has two benefits at 1M+ TPS:
/// 1. **Narrow writes** — each transition sends a minimal payload to Postgres, reducing
///    WAL volume and index churn.
/// 2. **Single insert point** — `save()` can be replaced by a Kafka producer (or any
///    CDC sink) without touching the rest of the call-graph.
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Insert the initial job row (Pending state).
    ///
    /// `messages_json` must be `None` on the entity — messages are stored in
    /// object storage, not in Postgres.  This is the **only** method that INSERTs.
    async fn save(&self, job: &InferenceJob) -> Result<()>;

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>>;

    async fn update_status(&self, job_id: &JobId, status: JobStatus) -> Result<()>;

    /// Atomically mark a job as Cancelled and record the exact cancellation timestamp.
    /// No-op if the job is already in a terminal state (Completed / Failed).
    async fn cancel_job(&self, job_id: &JobId, cancelled_at: DateTime<Utc>) -> Result<()>;

    /// Atomically mark a job as Failed with a machine-readable failure_reason.
    /// Only affects non-terminal jobs (Pending/Running).
    async fn fail_with_reason(
        &self,
        job_id: &JobId,
        reason: &str,
        error_msg: Option<&str>,
    ) -> Result<()>;

    /// Transition Pending → Running: record dispatch metadata in a single targeted UPDATE.
    async fn mark_running(
        &self,
        job_id: &JobId,
        started_at: DateTime<Utc>,
        provider_id: Option<Uuid>,
        queue_time_ms: i32,
    ) -> Result<()>;

    /// Transition Running → Completed: record all result columns in a single targeted UPDATE.
    #[allow(clippy::too_many_arguments)]
    async fn mark_completed(
        &self,
        job_id: &JobId,
        completed_at: DateTime<Utc>,
        result_text: Option<&str>,
        tool_calls_json: Option<&serde_json::Value>,
        latency_ms: i32,
        ttft_ms: Option<i32>,
        prompt_tokens: Option<i32>,
        completion_tokens: Option<i32>,
        cached_tokens: Option<i32>,
    ) -> Result<()>;

    /// Persist image object-storage keys after async upload completes.
    async fn update_image_keys(&self, job_id: &JobId, image_keys: Vec<String>) -> Result<()>;

    /// Return all jobs currently in Pending or Running state, ordered by created_at ASC.
    /// Used on startup to recover jobs that were in-flight when the server last stopped.
    async fn list_pending(&self) -> Result<Vec<InferenceJob>>;
}
