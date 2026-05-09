//! PostgreSQL implementation of [`InstallAttemptsLog`].
//!
//! Append-only history layer. The only non-trivial part is `start`: we
//! compute `attempt_no = COALESCE(MAX(attempt_no), 0) + 1` for the model in
//! the same INSERT so concurrent retries don't collide on the
//! `(model_id, attempt_no)` unique constraint.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::application::ports::outbound::install_attempts_log::{
    AttemptFailure, AttemptSuccess, AttemptsFilter, InstallAttemptsLog,
};
use crate::domain::entities::{
    AttemptStage, AttemptStatus, ErrorKind, InstallAttempt, TriggeredBy,
};

const ATTEMPT_COLS: &str = "id, model_id, attempt_no, started_at, finished_at, \
                            status, stage, error_kind, error_message, \
                            bytes_downloaded, total_bytes, duration_ms, \
                            retryable, triggered_by";

pub struct PostgresInstallAttemptsLog {
    pool: PgPool,
}

impl PostgresInstallAttemptsLog {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_status(s: &str) -> Result<AttemptStatus> {
    Ok(match s {
        "in_progress" => AttemptStatus::InProgress,
        "succeeded" => AttemptStatus::Succeeded,
        "failed" => AttemptStatus::Failed,
        "cancelled" => AttemptStatus::Cancelled,
        other => return Err(anyhow::anyhow!("invalid attempt status: {other}")),
    })
}

fn parse_stage(s: &str) -> Result<AttemptStage> {
    Ok(match s {
        "download" => AttemptStage::Download,
        "upload" => AttemptStage::Upload,
        "verify" => AttemptStage::Verify,
        other => return Err(anyhow::anyhow!("invalid stage: {other}")),
    })
}

fn parse_error_kind(s: &str) -> Result<ErrorKind> {
    Ok(match s {
        "network" => ErrorKind::Network,
        "rate_limit" => ErrorKind::RateLimit,
        "s3_error" => ErrorKind::S3Error,
        "sha256_mismatch" => ErrorKind::Sha256Mismatch,
        "timeout" => ErrorKind::Timeout,
        "startup_recovery" => ErrorKind::StartupRecovery,
        "unknown" => ErrorKind::Unknown,
        "source_404" => ErrorKind::Source404,
        "auth" => ErrorKind::Auth,
        "disk_full" => ErrorKind::DiskFull,
        "invalid_gguf" => ErrorKind::InvalidGguf,
        "quantization_unsupported" => ErrorKind::QuantizationUnsupported,
        other => return Err(anyhow::anyhow!("invalid error_kind: {other}")),
    })
}

fn parse_triggered_by(s: &str) -> Result<TriggeredBy> {
    Ok(match s {
        "register" => TriggeredBy::Register,
        "admin_retry" => TriggeredBy::AdminRetry,
        "patch_source" => TriggeredBy::PatchSource,
        other => return Err(anyhow::anyhow!("invalid triggered_by: {other}")),
    })
}

fn row_to_attempt(row: &sqlx::postgres::PgRow) -> Result<InstallAttempt> {
    use sqlx::Row as _;
    let status: String = row.try_get("status").context("status")?;
    let stage: Option<String> = row.try_get("stage").context("stage")?;
    let error_kind: Option<String> = row.try_get("error_kind").context("error_kind")?;
    let triggered_by: Option<String> =
        row.try_get("triggered_by").context("triggered_by")?;

    let bytes_downloaded: Option<i64> =
        row.try_get("bytes_downloaded").context("bytes_downloaded")?;
    let total_bytes: Option<i64> = row.try_get("total_bytes").context("total_bytes")?;
    let duration_ms: Option<i64> = row.try_get("duration_ms").context("duration_ms")?;

    Ok(InstallAttempt {
        id: row.try_get("id").context("id")?,
        model_id: row.try_get("model_id").context("model_id")?,
        attempt_no: row.try_get("attempt_no").context("attempt_no")?,
        started_at: row.try_get("started_at").context("started_at")?,
        finished_at: row
            .try_get::<Option<DateTime<Utc>>, _>("finished_at")
            .context("finished_at")?,
        status: parse_status(&status)?,
        stage: stage.as_deref().map(parse_stage).transpose()?,
        error_kind: error_kind.as_deref().map(parse_error_kind).transpose()?,
        error_message: row
            .try_get::<Option<String>, _>("error_message")
            .context("error_message")?,
        bytes_downloaded: bytes_downloaded.map(|n| n.max(0) as u64),
        total_bytes: total_bytes.map(|n| n.max(0) as u64),
        duration_ms: duration_ms.map(|n| n.max(0) as u64),
        retryable: row.try_get::<Option<bool>, _>("retryable").context("retryable")?,
        triggered_by: triggered_by
            .as_deref()
            .map(parse_triggered_by)
            .transpose()?,
    })
}

#[async_trait]
impl InstallAttemptsLog for PostgresInstallAttemptsLog {
    async fn start(&self, model_id: &str, triggered_by: TriggeredBy) -> Result<i64> {
        // attempt_no = MAX(attempt_no) + 1 computed inside the INSERT so
        // racing retries don't collide on the (model_id, attempt_no) UNIQUE.
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO veronex_model_install_attempts
                (model_id, attempt_no, status, triggered_by, started_at)
             VALUES (
                $1,
                COALESCE((SELECT MAX(attempt_no) FROM veronex_model_install_attempts WHERE model_id = $1), 0) + 1,
                'in_progress',
                $2,
                now()
             )
             RETURNING id",
        )
        .bind(model_id)
        .bind(triggered_by.as_str())
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("start attempt for {model_id}"))?;
        Ok(row.0)
    }

    async fn update_progress(
        &self,
        id: i64,
        stage: AttemptStage,
        bytes_done: u64,
        total: Option<u64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE veronex_model_install_attempts
             SET stage = $1, bytes_downloaded = $2, total_bytes = $3
             WHERE id = $4",
        )
        .bind(stage.as_str())
        .bind(bytes_done as i64)
        .bind(total.map(|n| n as i64))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn succeed(&self, id: i64, info: &AttemptSuccess) -> Result<()> {
        sqlx::query(
            "UPDATE veronex_model_install_attempts
             SET status = 'succeeded',
                 finished_at = now(),
                 bytes_downloaded = COALESCE($1, bytes_downloaded),
                 total_bytes = COALESCE($2, total_bytes),
                 duration_ms = $3
             WHERE id = $4",
        )
        .bind(info.bytes_downloaded.map(|n| n as i64))
        .bind(info.total_bytes.map(|n| n as i64))
        .bind(info.duration_ms.map(|n| n as i64))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn fail(&self, id: i64, info: &AttemptFailure) -> Result<()> {
        sqlx::query(
            "UPDATE veronex_model_install_attempts
             SET status = 'failed',
                 finished_at = now(),
                 stage = COALESCE($1, stage),
                 error_kind = $2,
                 error_message = $3,
                 retryable = $4,
                 bytes_downloaded = COALESCE($5, bytes_downloaded),
                 total_bytes = COALESCE($6, total_bytes),
                 duration_ms = $7
             WHERE id = $8",
        )
        .bind(info.stage.map(|s| s.as_str()))
        .bind(info.error_kind.as_str())
        .bind(&info.error_message)
        .bind(info.retryable)
        .bind(info.bytes_downloaded.map(|n| n as i64))
        .bind(info.total_bytes.map(|n| n as i64))
        .bind(info.duration_ms.map(|n| n as i64))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn cancel(&self, id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE veronex_model_install_attempts
             SET status = 'cancelled', finished_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list(&self, filter: &AttemptsFilter) -> Result<Vec<InstallAttempt>> {
        let limit = filter.limit.unwrap_or(50);
        let offset = filter.offset.unwrap_or(0);
        let q = format!(
            "SELECT {ATTEMPT_COLS} FROM veronex_model_install_attempts
             WHERE ($1::text IS NULL OR model_id = $1)
             ORDER BY started_at DESC
             LIMIT $2 OFFSET $3"
        );
        let rows = sqlx::query(&q)
            .bind(&filter.model_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_attempt).collect()
    }

    async fn delete(&self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM veronex_model_install_attempts WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    async fn delete_by_model(&self, model_id: &str) -> Result<u64> {
        let r = sqlx::query(
            "DELETE FROM veronex_model_install_attempts WHERE model_id = $1",
        )
        .bind(model_id)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected())
    }
}
