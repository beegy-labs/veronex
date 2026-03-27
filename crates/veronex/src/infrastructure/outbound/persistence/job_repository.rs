use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{ApiFormat, JobSource, JobStatus, ProviderType};
use crate::domain::value_objects::{JobId, ModelName, Prompt};

/// SELECT column list shared by `get()` and `list_pending()`.
/// Large content columns (prompt, result_text, messages_json, tool_calls_json)
/// are omitted — they live in S3. Only metadata columns remain.
const JOB_COLS: &str = "id, prompt_preview, model_name, provider_type, status, error, \
    created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, \
    prompt_tokens, completion_tokens, cached_tokens, source, provider_id, api_format, \
    request_path, conversation_id, queue_time_ms, \
    cancelled_at, messages_hash, messages_prefix_hash, failure_reason, image_keys, mcp_loop_id";

pub struct PostgresJobRepository {
    pool: PgPool,
}

impl PostgresJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── Row mapping ────────────────────────────────────────────────────────────────

fn row_to_job(row: &sqlx::postgres::PgRow) -> Result<InferenceJob> {
    use sqlx::Row;

    let id: Uuid = row.try_get("id").context("missing column: id")?;
    let prompt_preview: Option<String> = row.try_get("prompt_preview").unwrap_or(None);
    let model_name: String = row
        .try_get("model_name")
        .context("missing column: model_name")?;
    let provider_str: String = row
        .try_get("provider_type")
        .context("missing column: provider_type")?;
    let status_str: String = row
        .try_get("status")
        .context("missing column: status")?;
    let error: Option<String> = row.try_get("error").context("missing column: error")?;
    let created_at: DateTime<Utc> = row
        .try_get("created_at")
        .context("missing column: created_at")?;
    let started_at: Option<DateTime<Utc>> = row
        .try_get("started_at")
        .context("missing column: started_at")?;
    let completed_at: Option<DateTime<Utc>> = row
        .try_get("completed_at")
        .context("missing column: completed_at")?;
    let api_key_id: Option<Uuid> = row.try_get("api_key_id").unwrap_or(None);
    let account_id: Option<Uuid> = row.try_get("account_id").unwrap_or(None);
    let latency_ms: Option<i32> = row.try_get("latency_ms").unwrap_or(None);
    let ttft_ms: Option<i32> = row.try_get("ttft_ms").unwrap_or(None);
    let prompt_tokens: Option<i32> = row.try_get("prompt_tokens").unwrap_or(None);
    let completion_tokens: Option<i32> = row.try_get("completion_tokens").unwrap_or(None);
    let cached_tokens: Option<i32> = row.try_get("cached_tokens").unwrap_or(None);
    let source_str: String = row.try_get("source").unwrap_or_else(|_| "api".to_string());
    let api_format_str: String = row.try_get("api_format").unwrap_or_else(|_| "openai_compat".to_string());
    let provider_id: Option<Uuid> = row.try_get("provider_id").unwrap_or(None);
    let request_path: Option<String> = row.try_get("request_path").unwrap_or(None);
    let conversation_id: Option<String> = row.try_get("conversation_id").unwrap_or(None);
    let queue_time_ms: Option<i32> = row.try_get("queue_time_ms").unwrap_or(None);
    let cancelled_at: Option<DateTime<Utc>> = row.try_get("cancelled_at").unwrap_or(None);
    let messages_hash: Option<String> = row.try_get("messages_hash").unwrap_or(None);
    let messages_prefix_hash: Option<String> = row.try_get("messages_prefix_hash").unwrap_or(None);
    let failure_reason: Option<String> = row.try_get("failure_reason").unwrap_or(None);
    let image_keys: Option<Vec<String>> = row.try_get("image_keys").unwrap_or(None);
    let mcp_loop_id: Option<Uuid> = row.try_get("mcp_loop_id").unwrap_or(None);

    // Reconstruct Prompt from preview (in-memory placeholder — full prompt is in S3)
    let prompt_str = prompt_preview.as_deref().unwrap_or("");

    Ok(InferenceJob {
        id: JobId(id),
        prompt: Prompt::new(prompt_str)?,
        prompt_preview,
        model_name: ModelName::new(&model_name)?,
        provider_type: provider_str.parse::<ProviderType>().map_err(|e| anyhow::anyhow!(e))?,
        status: status_str.parse::<JobStatus>().map_err(|e| anyhow::anyhow!(e))?,
        error,
        created_at,
        started_at,
        completed_at,
        result_text: None,
        api_key_id,
        account_id,
        latency_ms,
        ttft_ms,
        prompt_tokens,
        completion_tokens,
        cached_tokens,
        source: source_str.parse::<JobSource>().unwrap_or_default(),
        provider_id,
        api_format: api_format_str.parse::<ApiFormat>().unwrap_or_default(),
        messages: None,
        tools: None,
        request_path,
        queue_time_ms,
        cancelled_at,
        conversation_id,
        tool_calls_json: None,
        messages_hash,
        messages_prefix_hash,
        failure_reason,
        images: None,
        image_keys,
        stop: None,
        seed: None,
        response_format: None,
        frequency_penalty: None,
        presence_penalty: None,
        mcp_loop_id,
    })
}

// ── Repository impl ────────────────────────────────────────────────────────────

#[async_trait]
impl JobRepository for PostgresJobRepository {
    /// Insert the initial job row (Pending state).
    ///
    /// Only metadata + prompt_preview are stored. Large content (full prompt,
    /// messages, result, tool_calls) is written to S3 at finalize time.
    async fn save(&self, job: &InferenceJob) -> Result<()> {
        use crate::domain::services::message_hashing::compute_message_hashes;

        let (messages_hash, messages_prefix_hash) = match (&job.messages_hash, &job.messages_prefix_hash) {
            (Some(h), Some(p)) => (Some(h.clone()), Some(p.clone())),
            _ => job.messages
                .as_ref()
                .and_then(compute_message_hashes)
                .map(|(h, p)| (Some(h), Some(p)))
                .unwrap_or((None, None)),
        };

        sqlx::query(
            "INSERT INTO inference_jobs
                 (id, prompt_preview, model_name, provider_type, status, error,
                  created_at, started_at, completed_at, api_key_id, account_id,
                  latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens,
                  source, provider_id, api_format, request_path, conversation_id,
                  queue_time_ms, cancelled_at, messages_hash, messages_prefix_hash,
                  failure_reason, image_keys, mcp_loop_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(job.id.0)
        .bind(&job.prompt_preview)
        .bind(job.model_name.as_str())
        .bind(job.provider_type.as_str())
        .bind(job.status.as_str())
        .bind(&job.error)
        .bind(job.created_at)
        .bind(job.started_at)
        .bind(job.completed_at)
        .bind(job.api_key_id)
        .bind(job.account_id)
        .bind(job.latency_ms)
        .bind(job.ttft_ms)
        .bind(job.prompt_tokens)
        .bind(job.completion_tokens)
        .bind(job.cached_tokens)
        .bind(job.source.as_str())
        .bind(job.provider_id)
        .bind(job.api_format.as_str())
        .bind(&job.request_path)
        .bind(&job.conversation_id)
        .bind(job.queue_time_ms)
        .bind(job.cancelled_at)
        .bind(messages_hash)
        .bind(messages_prefix_hash)
        .bind(&job.failure_reason)
        .bind(&job.image_keys)
        .bind(job.mcp_loop_id)
        .execute(&self.pool)
        .await
        .context("failed to save inference job")?;

        Ok(())
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>> {
        let row = sqlx::query(&format!(
            "SELECT {JOB_COLS} FROM inference_jobs WHERE id = $1"
        ))
        .bind(job_id.0)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get inference job")?;

        match row {
            Some(r) => Ok(Some(row_to_job(&r)?)),
            None => Ok(None),
        }
    }

    async fn update_status(&self, job_id: &JobId, status: JobStatus) -> Result<()> {
        sqlx::query("UPDATE inference_jobs SET status = $2 WHERE id = $1")
            .bind(job_id.0)
            .bind(status.as_str())
            .execute(&self.pool)
            .await
            .context("failed to update job status")?;

        Ok(())
    }

    async fn cancel_job(&self, job_id: &JobId, cancelled_at: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            "UPDATE inference_jobs
             SET status = 'cancelled', cancelled_at = $2
             WHERE id = $1
               AND status NOT IN ('completed', 'failed')",
        )
        .bind(job_id.0)
        .bind(cancelled_at)
        .execute(&self.pool)
        .await
        .context("failed to cancel inference job")?;

        Ok(())
    }

    async fn fail_with_reason(
        &self,
        job_id: &JobId,
        reason: &str,
        error_msg: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE inference_jobs
             SET status = 'failed', failure_reason = $2, error = COALESCE($3, error)
             WHERE id = $1
               AND status NOT IN ('completed', 'failed', 'cancelled')",
        )
        .bind(job_id.0)
        .bind(reason)
        .bind(error_msg)
        .execute(&self.pool)
        .await
        .context("failed to mark job as failed with reason")?;
        Ok(())
    }

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
    ) -> Result<()> {
        sqlx::query(
            "UPDATE inference_jobs
             SET status            = 'completed',
                 started_at        = COALESCE($2, started_at),
                 completed_at      = $3,
                 provider_id       = COALESCE($4, provider_id),
                 queue_time_ms     = COALESCE($5, queue_time_ms),
                 latency_ms        = $6,
                 ttft_ms           = $7,
                 prompt_tokens     = $8,
                 completion_tokens = $9,
                 cached_tokens     = $10,
                 has_tool_calls    = $11
             WHERE id = $1
               AND status NOT IN ('cancelled', 'failed')",
        )
        .bind(job_id.0)
        .bind(started_at)
        .bind(completed_at)
        .bind(provider_id)
        .bind(queue_time_ms)
        .bind(latency_ms)
        .bind(ttft_ms)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(cached_tokens)
        .bind(has_tool_calls)
        .execute(&self.pool)
        .await
        .context("failed to finalize inference job")?;
        Ok(())
    }

    async fn update_image_keys(&self, job_id: &JobId, image_keys: Vec<String>) -> Result<()> {
        sqlx::query(
            "UPDATE inference_jobs SET image_keys = $2 WHERE id = $1",
        )
        .bind(job_id.0)
        .bind(&image_keys)
        .execute(&self.pool)
        .await
        .context("failed to update image_keys")?;
        Ok(())
    }

    async fn list_pending(&self) -> Result<Vec<InferenceJob>> {
        let rows = sqlx::query(&format!(
            "SELECT {JOB_COLS} FROM inference_jobs \
             WHERE status IN ('pending', 'running') \
             ORDER BY created_at ASC LIMIT 1000"
        ))
        .fetch_all(&self.pool)
        .await
        .context("failed to list pending jobs")?;

        rows.iter().map(row_to_job).collect()
    }
}
