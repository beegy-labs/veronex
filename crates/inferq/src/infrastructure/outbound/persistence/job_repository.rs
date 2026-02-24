use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::job_repository::JobRepository;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{BackendType, JobStatus};
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
                 (id, prompt, model_name, backend, status, error, created_at, started_at, completed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET
                 status       = EXCLUDED.status,
                 error        = EXCLUDED.error,
                 started_at   = EXCLUDED.started_at,
                 completed_at = EXCLUDED.completed_at",
        )
        .bind(job.id.0)
        .bind(job.prompt.as_str())
        .bind(job.model_name.as_str())
        .bind(backend_to_str(&job.backend))
        .bind(status_to_str(job.status))
        .bind(&job.error)
        .bind(job.created_at)
        .bind(job.started_at)
        .bind(job.completed_at)
        .execute(&self.pool)
        .await
        .context("failed to save inference job")?;

        Ok(())
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<InferenceJob>> {
        let row = sqlx::query(
            "SELECT id, prompt, model_name, backend, status, error, created_at, started_at, completed_at
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
            "SELECT id, prompt, model_name, backend, status, error, created_at, started_at, completed_at
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
