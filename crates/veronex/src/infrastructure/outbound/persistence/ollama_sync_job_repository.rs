use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::ollama_sync_job_repository::{
    OllamaSyncJob, OllamaSyncJobRepository,
};

pub struct PostgresOllamaSyncJobRepository {
    pool: PgPool,
}

impl PostgresOllamaSyncJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OllamaSyncJobRepository for PostgresOllamaSyncJobRepository {
    async fn create(&self, total_providers: i32) -> Result<Uuid> {
        let id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO ollama_sync_jobs (id, total_providers)
            VALUES ($1, $2)
            "#,
            id,
            total_providers,
        )
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    async fn update_progress(&self, id: Uuid, result: serde_json::Value) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE ollama_sync_jobs
            SET done_providers = done_providers + 1,
                results = results || jsonb_build_array($2::jsonb)
            WHERE id = $1
            "#,
            id,
            result,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn complete(&self, id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE ollama_sync_jobs
            SET status = 'completed', completed_at = NOW()
            WHERE id = $1
            "#,
            id,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_latest(&self) -> Result<Option<OllamaSyncJob>> {
        let row = sqlx::query!(
            r#"
            SELECT id, started_at, completed_at, status, total_providers, done_providers, results
            FROM ollama_sync_jobs
            ORDER BY started_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| OllamaSyncJob {
            id: r.id,
            started_at: r.started_at,
            completed_at: r.completed_at,
            status: r.status,
            total_providers: r.total_providers,
            done_providers: r.done_providers,
            results: r.results,
        }))
    }
}
