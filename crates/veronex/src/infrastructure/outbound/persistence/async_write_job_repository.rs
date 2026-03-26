use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::JobStatus;
use crate::domain::value_objects::JobId;

// ── Write operation enum ────────────────────────────────────────────────────

enum WriteOp {
    MarkRunning {
        job_id: JobId,
        started_at: DateTime<Utc>,
        provider_id: Option<Uuid>,
        queue_time_ms: i32,
    },
    MarkCompleted {
        job_id: JobId,
        completed_at: DateTime<Utc>,
        result_text: Option<String>,
        tool_calls_json: Option<serde_json::Value>,
        latency_ms: i32,
        ttft_ms: Option<i32>,
        prompt_tokens: Option<i32>,
        completion_tokens: Option<i32>,
        cached_tokens: Option<i32>,
    },
    FailWithReason {
        job_id: JobId,
        reason: String,
        error_msg: Option<String>,
    },
    CancelJob {
        job_id: JobId,
        cancelled_at: DateTime<Utc>,
    },
    UpdateImageKeys {
        job_id: JobId,
        image_keys: Vec<String>,
    },
}

// ── AsyncWriteJobRepository ─────────────────────────────────────────────────

/// Off-critical-path write buffer for [`JobRepository`].
///
/// # Write discipline
///
/// * **`save()`** — synchronous. This is the single INSERT and the future Kafka
///   producer insertion point. Callers get a DB-committed row before the job_id
///   is returned, so subsequent targeted UPDATEs never race against a missing row.
///
/// * **All state-transition writes** (`mark_running`, `mark_completed`,
///   `fail_with_reason`, `cancel_job`, `update_image_keys`) — enqueued to an
///   MPSC channel and return `Ok(())` immediately. A background task drains the
///   channel in batches (up to `batch_size` ops, executed concurrently) and
///   writes to Postgres out-of-band.
///
///   Race-safety: state-transition writes arrive minutes after `save()`, so a
///   batch can never contain both a `save()` and a subsequent UPDATE for the same
///   job — the INSERT is always already committed by the time dispatch occurs.
///
/// * **`get()`, `list_pending()`, `update_status()`** — synchronous delegation
///   to the inner repo (reads and startup recovery require real data).
///
/// # Failure semantics
///
/// If the channel is full (`try_send` fails), the write is logged and dropped.
/// Job state is authoritative in DashMap during execution; Postgres is the
/// recovery store. Dropped writes are recoverable via `list_pending()` on restart.
pub struct AsyncWriteJobRepository {
    tx: mpsc::Sender<WriteOp>,
    inner: Arc<dyn JobRepository>,
}

impl AsyncWriteJobRepository {
    /// Spawn the background writer and return the repository handle.
    ///
    /// Recommended defaults: `channel_capacity = 65_536`, `batch_size = 256`.
    pub fn new(
        inner: Arc<dyn JobRepository>,
        channel_capacity: usize,
        batch_size: usize,
    ) -> Self {
        let (tx, rx) = mpsc::channel(channel_capacity);
        tokio::spawn(batch_writer(rx, inner.clone(), batch_size));
        Self { tx, inner }
    }

    fn enqueue(&self, op: WriteOp) {
        if let Err(e) = self.tx.try_send(op) {
            tracing::warn!("job write channel full/closed — write dropped: {e}");
        }
    }
}

// ── Background batch writer ─────────────────────────────────────────────────

async fn batch_writer(
    mut rx: mpsc::Receiver<WriteOp>,
    repo: Arc<dyn JobRepository>,
    batch_size: usize,
) {
    loop {
        // Block until at least one op is available.
        let Some(first) = rx.recv().await else { break };

        let mut batch = Vec::with_capacity(batch_size);
        batch.push(first);

        // Drain additional ops without waiting (up to batch_size).
        while batch.len() < batch_size {
            match rx.try_recv() {
                Ok(op) => batch.push(op),
                Err(_) => break,
            }
        }

        // Execute batch concurrently — each op is a single-row targeted UPDATE
        // (idempotent, no cross-job dependencies within a batch).
        let handles: Vec<_> = batch
            .into_iter()
            .map(|op| tokio::spawn(execute_op(repo.clone(), op)))
            .collect();

        for h in handles {
            if let Err(e) = h.await {
                tracing::warn!("write op task panicked: {e}");
            }
        }
    }
}

async fn execute_op(repo: Arc<dyn JobRepository>, op: WriteOp) {
    match op {
        WriteOp::MarkRunning { job_id, started_at, provider_id, queue_time_ms } => {
            if let Err(e) = repo.mark_running(&job_id, started_at, provider_id, queue_time_ms).await {
                tracing::error!(job_id = %job_id.0, "async write mark_running failed: {e}");
            }
        }
        WriteOp::MarkCompleted {
            job_id, completed_at, result_text, tool_calls_json,
            latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens,
        } => {
            if let Err(e) = repo.mark_completed(
                &job_id, completed_at,
                result_text.as_deref(), tool_calls_json.as_ref(),
                latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens,
            ).await {
                tracing::error!(job_id = %job_id.0, "async write mark_completed failed: {e}");
            }
        }
        WriteOp::FailWithReason { job_id, reason, error_msg } => {
            if let Err(e) = repo.fail_with_reason(&job_id, &reason, error_msg.as_deref()).await {
                tracing::error!(job_id = %job_id.0, "async write fail_with_reason failed: {e}");
            }
        }
        WriteOp::CancelJob { job_id, cancelled_at } => {
            if let Err(e) = repo.cancel_job(&job_id, cancelled_at).await {
                tracing::error!(job_id = %job_id.0, "async write cancel_job failed: {e}");
            }
        }
        WriteOp::UpdateImageKeys { job_id, image_keys } => {
            if let Err(e) = repo.update_image_keys(&job_id, image_keys).await {
                tracing::error!(job_id = %job_id.0, "async write update_image_keys failed: {e}");
            }
        }
    }
}

// ── JobRepository impl ──────────────────────────────────────────────────────

#[async_trait]
impl JobRepository for AsyncWriteJobRepository {
    /// Synchronous INSERT — this is the single write point and future Kafka insertion.
    async fn save(&self, job: &InferenceJob) -> Result<()> {
        self.inner.save(job).await
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>> {
        self.inner.get(job_id).await
    }

    /// Synchronous — used in startup recovery; ordering relative to re-enqueue matters.
    async fn update_status(&self, job_id: &JobId, status: JobStatus) -> Result<()> {
        self.inner.update_status(job_id, status).await
    }

    async fn cancel_job(&self, job_id: &JobId, cancelled_at: DateTime<Utc>) -> Result<()> {
        self.enqueue(WriteOp::CancelJob { job_id: job_id.clone(), cancelled_at });
        Ok(())
    }

    async fn fail_with_reason(&self, job_id: &JobId, reason: &str, error_msg: Option<&str>) -> Result<()> {
        self.enqueue(WriteOp::FailWithReason {
            job_id: job_id.clone(),
            reason: reason.to_string(),
            error_msg: error_msg.map(str::to_string),
        });
        Ok(())
    }

    async fn mark_running(
        &self,
        job_id: &JobId,
        started_at: DateTime<Utc>,
        provider_id: Option<Uuid>,
        queue_time_ms: i32,
    ) -> Result<()> {
        self.enqueue(WriteOp::MarkRunning {
            job_id: job_id.clone(),
            started_at,
            provider_id,
            queue_time_ms,
        });
        Ok(())
    }

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
    ) -> Result<()> {
        self.enqueue(WriteOp::MarkCompleted {
            job_id: job_id.clone(),
            completed_at,
            result_text: result_text.map(str::to_string),
            tool_calls_json: tool_calls_json.cloned(),
            latency_ms,
            ttft_ms,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
        });
        Ok(())
    }

    async fn update_image_keys(&self, job_id: &JobId, image_keys: Vec<String>) -> Result<()> {
        self.enqueue(WriteOp::UpdateImageKeys { job_id: job_id.clone(), image_keys });
        Ok(())
    }

    async fn list_pending(&self) -> Result<Vec<InferenceJob>> {
        self.inner.list_pending().await
    }
}
