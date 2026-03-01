use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{ApiFormat, BackendType, JobSource, JobStatus};
use crate::domain::value_objects::{JobId, ModelName, Prompt};

pub struct PostgresJobRepository {
    pool: PgPool,
}

impl PostgresJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── Enum conversions ───────────────────────────────────────────────────────────

fn backend_to_str(b: &BackendType) -> &'static str {
    match b {
        BackendType::Ollama => "ollama",
        BackendType::Gemini => "gemini",
    }
}

fn str_to_backend(s: &str) -> Result<BackendType> {
    match s {
        "ollama" => Ok(BackendType::Ollama),
        "gemini" => Ok(BackendType::Gemini),
        _ => Err(anyhow::anyhow!("unknown backend type: {s}")),
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
        .try_get("backend")
        .context("missing column: backend")?;
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
    let backend_id: Option<Uuid> = row.try_get("backend_id").unwrap_or(None);
    let request_path: Option<String> = row.try_get("request_path").unwrap_or(None);

    Ok(InferenceJob {
        id: JobId(id),
        prompt: Prompt::new(&prompt)?,
        model_name: ModelName::new(&model_name)?,
        backend: str_to_backend(&backend_str)?,
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
        backend_id,
        api_format: str_to_api_format(&api_format_str),
        messages: None,
        request_path,
    })
}

// ── Repository impl ────────────────────────────────────────────────────────────

#[async_trait]
impl JobRepository for PostgresJobRepository {
    /// Insert or update the full job record (upsert).
    ///
    /// Safe to call on both initial save and subsequent status transitions
    /// because immutable fields (prompt, model_name, backend, created_at)
    /// are excluded from the ON CONFLICT update clause.
    async fn save(&self, job: &InferenceJob) -> Result<()> {
        sqlx::query(
            "INSERT INTO inference_jobs
                 (id, prompt, model_name, backend, status, error, result_text, created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens, source, backend_id, api_format, request_path)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)
             ON CONFLICT (id) DO UPDATE SET
                 status            = EXCLUDED.status,
                 error             = EXCLUDED.error,
                 result_text       = COALESCE(EXCLUDED.result_text, inference_jobs.result_text),
                 started_at        = EXCLUDED.started_at,
                 completed_at      = EXCLUDED.completed_at,
                 latency_ms        = COALESCE(EXCLUDED.latency_ms, inference_jobs.latency_ms),
                 ttft_ms           = COALESCE(EXCLUDED.ttft_ms, inference_jobs.ttft_ms),
                 prompt_tokens     = COALESCE(EXCLUDED.prompt_tokens, inference_jobs.prompt_tokens),
                 completion_tokens = COALESCE(EXCLUDED.completion_tokens, inference_jobs.completion_tokens),
                 cached_tokens     = COALESCE(EXCLUDED.cached_tokens, inference_jobs.cached_tokens),
                 backend_id        = COALESCE(EXCLUDED.backend_id, inference_jobs.backend_id)",
        )
        .bind(job.id.0)
        .bind(job.prompt.as_str())
        .bind(job.model_name.as_str())
        .bind(backend_to_str(&job.backend))
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
        .bind(job.backend_id)
        .bind(api_format_to_str(job.api_format))
        .bind(&job.request_path)
        .execute(&self.pool)
        .await
        .context("failed to save inference job")?;

        Ok(())
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>> {
        let row = sqlx::query(
            "SELECT id, prompt, model_name, backend, status, error, result_text, created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens, source, backend_id, api_format, request_path
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

    async fn list_pending(&self) -> Result<Vec<InferenceJob>> {
        let rows = sqlx::query(
            "SELECT id, prompt, model_name, backend, status, error, result_text, created_at, started_at, completed_at, api_key_id, account_id, latency_ms, ttft_ms, prompt_tokens, completion_tokens, cached_tokens, source, backend_id, api_format, request_path
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
