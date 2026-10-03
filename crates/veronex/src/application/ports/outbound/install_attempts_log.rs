//! Outbound port — install attempt history (Phase 2).
//!
//! Append-only log of every install attempt for every Modelfile. Rows are
//! NOT cascaded when the model is deleted — admin manually trims via the
//! delete endpoints. This is intentional: install failures are operational
//! evidence and should outlive the row that caused them.

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::entities::{
    AttemptStage, ErrorKind, InstallAttempt, TriggeredBy,
};

/// Filter for [`InstallAttemptsLog::list`].
#[derive(Debug, Default, Clone)]
pub struct AttemptsFilter {
    pub model_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Successful-completion bookkeeping. Kept as a struct rather than a long
/// argument list so the orchestrator can build it incrementally without
/// positional-argument confusion.
#[derive(Debug, Clone, Default)]
pub struct AttemptSuccess {
    pub bytes_downloaded: Option<u64>,
    pub total_bytes: Option<u64>,
    pub duration_ms: Option<u64>,
}

/// Failed-completion bookkeeping. `retryable` is the operator-facing hint —
/// Veronex never auto-retries.
#[derive(Debug, Clone)]
pub struct AttemptFailure {
    pub stage: Option<AttemptStage>,
    pub error_kind: ErrorKind,
    pub error_message: String,
    pub retryable: bool,
    pub bytes_downloaded: Option<u64>,
    pub total_bytes: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[async_trait]
pub trait InstallAttemptsLog: Send + Sync {
    /// Open a new attempt row in `in_progress` state. Returns the row id.
    /// Implementation must compute `attempt_no = max(attempt_no) + 1` for
    /// this `model_id` atomically.
    async fn start(
        &self,
        model_id: &str,
        triggered_by: TriggeredBy,
    ) -> Result<i64>;

    /// Update the in-flight bytes counter. Called periodically by the
    /// download/upload pipelines so the SSE stream can serve progress.
    async fn update_progress(
        &self,
        id: i64,
        stage: AttemptStage,
        bytes_done: u64,
        total: Option<u64>,
    ) -> Result<()>;

    /// Mark an attempt as successfully finished.
    async fn succeed(&self, id: i64, info: &AttemptSuccess) -> Result<()>;

    /// Mark an attempt as failed.
    async fn fail(&self, id: i64, info: &AttemptFailure) -> Result<()>;

    /// Mark an attempt as cancelled (caller invoked the cancel API or the
    /// orchestrator aborted on shutdown).
    async fn cancel(&self, id: i64) -> Result<()>;

    /// List recent attempts, optionally filtered by model_id.
    async fn list(&self, filter: &AttemptsFilter) -> Result<Vec<InstallAttempt>>;

    /// Delete a single attempt row by id. Returns `Ok(false)` if it didn't
    /// exist. Admin manual cleanup only.
    async fn delete(&self, id: i64) -> Result<bool>;

    /// Delete all attempts for a model_id. Returns the number of rows
    /// removed. Admin manual cleanup only.
    async fn delete_by_model(&self, model_id: &str) -> Result<u64>;
}
