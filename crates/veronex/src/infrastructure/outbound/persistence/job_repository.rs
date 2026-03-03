use anyhow::{Context, Result};
use async_trait::async_trait;
use blake2::{Blake2b, Digest, digest::consts::U32};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{ApiFormat, ProviderType, JobSource, JobStatus};
use crate::domain::value_objects::{JobId, ModelName, Prompt};

type Blake2b256 = Blake2b<U32>;

/// Compute Blake2b-256 of the full messages array and its prefix (all-but-last).
///
/// Returns `(messages_hash, messages_prefix_hash)`.
/// `messages_prefix_hash` is an empty string for single-turn jobs (first message only).
/// Returns `None` when `messages` is None or not a JSON array.
fn compute_message_hashes(messages: &serde_json::Value) -> Option<(String, String)> {
    let arr = messages.as_array()?;
    if arr.is_empty() {
        return None;
    }

    // Full hash — identity of this job's complete context snapshot.
    let full_json = serde_json::to_string(arr).ok()?;
    let mut h = Blake2b256::new();
    h.update(full_json.as_bytes());
    let messages_hash = hex::encode(h.finalize());

    // Prefix hash — all turns except the last user message.
    // Empty string signals "first turn" so the grouping loop can skip parent lookup.
    let messages_prefix_hash = if arr.len() <= 1 {
        String::new()
    } else {
        let prefix = &arr[..arr.len() - 1];
        let prefix_json = serde_json::to_string(prefix).ok()?;
        let mut h = Blake2b256::new();
        h.update(prefix_json.as_bytes());
        hex::encode(h.finalize())
    };

    Some((messages_hash, messages_prefix_hash))
}

pub struct PostgresJobRepository {
    pool: PgPool,
}

impl PostgresJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── Enum conversions ───────────────────────────────────────────────────────────

fn provider_type_to_str(b: &ProviderType) -> &'static str {
    match b {
        ProviderType::Ollama => "ollama",
        ProviderType::Gemini => "gemini",
    }
}

fn str_to_provider_type(s: &str) -> Result<ProviderType> {
    match s {
        "ollama" => Ok(ProviderType::Ollama),
        "gemini" => Ok(ProviderType::Gemini),
        _ => Err(anyhow::anyhow!("unknown provider type: {s}")),
    }
}

fn status_to_str(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

fn str_to_status(s: &str) -> Result<JobStatus> {
    match s {
        "pending" => Ok(JobStatus::Pending),
        "running" => Ok(JobStatus::Running),
        "completed" => Ok(JobStatus::Completed),
        "failed" => Ok(JobStatus::Failed),
        "cancelled" => Ok(JobStatus::Cancelled),
        _ => Err(anyhow::anyhow!("unknown job status: {s}")),
    }
}

fn str_to_source(s: &str) -> JobSource {
    match s {
        "test" => JobSource::Test,
        _ => JobSource::Api,
    }
}

fn source_to_str(s: JobSource) -> &'static str {
    match s {
        JobSource::Api => "api",
        JobSource::Test => "test",
    }
}

fn api_format_to_str(f: ApiFormat) -> &'static str {
    match f {
        ApiFormat::OpenaiCompat => "openai_compat",
        ApiFormat::OllamaNative => "ollama_native",
        ApiFormat::GeminiNative => "gemini_native",
        ApiFormat::VeronexNative => "veronex_native",
    }
}

fn str_to_api_format(s: &str) -> ApiFormat {
    match s {
        "ollama_native" => ApiFormat::OllamaNative,
        "gemini_native" => ApiFormat::GeminiNative,
        "veronex_native" => ApiFormat::VeronexNative,
        _ => ApiFormat::OpenaiCompat,
    }
}

// ── Row mapping ────────────────────────────────────────────────────────────────

fn row_to_job(row: &sqlx::postgres::PgRow) -> Result<InferenceJob> {
    use sqlx::Row;

    let id: Uuid = row.try_get("id").context("missing column: id")?;
    let prompt: String = row.try_get("prompt").context("missing column: prompt")?;
    let model_name: String = row
        .try_get("model_name")
        .context("missing column: model_name")?;
    let backend_str: String = row
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
    let result_text: Option<String> = row
        .try_get("result_text")
        .context("missing column: result_text")?;
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
    let tool_calls_json: Option<serde_json::Value> = row.try_get("tool_calls_json").unwrap_or(None);
    let messages_json: Option<serde_json::Value> = row.try_get("messages_json").unwrap_or(None);
    let queue_time_ms: Option<i32> = row.try_get("queue_time_ms").unwrap_or(None);
    let cancelled_at: Option<DateTime<Utc>> = row.try_get("cancelled_at").unwrap_or(None);

    Ok(InferenceJob {
        id: JobId(id),
        prompt: Prompt::new(&prompt)?,
        model_name: ModelName::new(&model_name)?,
        provider_type: str_to_provider_type(&backend_str)?,
        status: str_to_status(&status_str)?,
        error,
        created_at,
        started_at,
        completed_at,
        result_text,
        api_key_id,
        account_id,
        latency_ms,
        ttft_ms,
        prompt_tokens,
        completion_tokens,
        cached_tokens,
        source: str_to_source(&source_str),
        provider_id,
        api_format: str_to_api_format(&api_format_str),
        messages: messages_json, // persisted as messages_json (migration 000045)
        tools: None,             // not persisted — in-memory only during dispatch
        request_path,
        queue_time_ms,
        cancelled_at,
        conversation_id,
        tool_calls_json,
    })
}

// ── Repository impl ────────────────────────────────────────────────────────────

#[async_trait]
impl JobRepository for PostgresJobRepository {
    /// Insert or update the full job record (upsert).
    ///
    /// Safe to call on both initial save and subsequent status transitions
    /// because immutable fields (prompt, model_name, provider, created_at)
    /// are excluded from the ON CONFLICT update clause.
    async fn save(&self, job: &InferenceJob) -> Result<()> {
        let (messages_hash, messages_prefix_hash) = job.messages
            .as_ref()
            .and_then(|m| compute_message_hashes(m))
            .map(|(h, p)| (Some(h), Some(p)))
            .unwrap_or((None, None));

        sqlx::query(
            "INSERT INTO inference_jobs
                 (id, prompt, model_name, provider_type, status, error, result_text, created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens, source, provider_id, api_format, request_path, conversation_id, tool_calls_json, messages_json, queue_time_ms, cancelled_at, messages_hash, messages_prefix_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
             ON CONFLICT (id) DO UPDATE SET
                 status                = EXCLUDED.status,
                 error                 = EXCLUDED.error,
                 result_text           = COALESCE(EXCLUDED.result_text, inference_jobs.result_text),
                 started_at            = EXCLUDED.started_at,
                 completed_at          = EXCLUDED.completed_at,
                 latency_ms            = COALESCE(EXCLUDED.latency_ms, inference_jobs.latency_ms),
                 ttft_ms               = COALESCE(EXCLUDED.ttft_ms, inference_jobs.ttft_ms),
                 prompt_tokens         = COALESCE(EXCLUDED.prompt_tokens, inference_jobs.prompt_tokens),
                 completion_tokens     = COALESCE(EXCLUDED.completion_tokens, inference_jobs.completion_tokens),
                 cached_tokens         = COALESCE(EXCLUDED.cached_tokens, inference_jobs.cached_tokens),
                 provider_id           = COALESCE(EXCLUDED.provider_id, inference_jobs.provider_id),
                 tool_calls_json       = COALESCE(EXCLUDED.tool_calls_json, inference_jobs.tool_calls_json),
                 messages_json         = COALESCE(EXCLUDED.messages_json, inference_jobs.messages_json),
                 queue_time_ms         = COALESCE(EXCLUDED.queue_time_ms, inference_jobs.queue_time_ms),
                 cancelled_at          = COALESCE(EXCLUDED.cancelled_at, inference_jobs.cancelled_at),
                 messages_hash         = COALESCE(EXCLUDED.messages_hash, inference_jobs.messages_hash),
                 messages_prefix_hash  = COALESCE(EXCLUDED.messages_prefix_hash, inference_jobs.messages_prefix_hash)",
        )
        .bind(job.id.0)
        .bind(job.prompt.as_str())
        .bind(job.model_name.as_str())
        .bind(provider_type_to_str(&job.provider_type))
        .bind(status_to_str(job.status))
        .bind(&job.error)
        .bind(&job.result_text)
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
        .bind(source_to_str(job.source))
        .bind(job.provider_id)
        .bind(api_format_to_str(job.api_format))
        .bind(&job.request_path)
        .bind(&job.conversation_id)
        .bind(&job.tool_calls_json)
        .bind(&job.messages)   // full input context (messages_json)
        .bind(job.queue_time_ms)
        .bind(job.cancelled_at)
        .bind(messages_hash)
        .bind(messages_prefix_hash)
        .execute(&self.pool)
        .await
        .context("failed to save inference job")?;

        Ok(())
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>> {
        let row = sqlx::query(
            "SELECT id, prompt, model_name, provider_type, status, error, result_text, created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens, source, provider_id, api_format, request_path, conversation_id, tool_calls_json, messages_json, queue_time_ms, cancelled_at
             FROM inference_jobs
             WHERE id = $1",
        )
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
            .bind(status_to_str(status))
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

    async fn list_pending(&self) -> Result<Vec<InferenceJob>> {
        let rows = sqlx::query(
            "SELECT id, prompt, model_name, provider_type, status, error, result_text, created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens, source, provider_id, api_format, request_path, conversation_id, tool_calls_json, messages_json, queue_time_ms, cancelled_at
             FROM inference_jobs
             WHERE status IN ('pending', 'running')
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list pending jobs")?;

        rows.iter().map(|r| row_to_job(r)).collect()
    }
}
