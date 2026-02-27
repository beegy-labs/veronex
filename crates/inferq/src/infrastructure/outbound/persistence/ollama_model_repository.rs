use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::ollama_model_repository::{
    OllamaBackendForModel, OllamaModelRepository, OllamaModelWithCount,
};

pub struct PostgresOllamaModelRepository {
    pool: PgPool,
}

impl PostgresOllamaModelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OllamaModelRepository for PostgresOllamaModelRepository {
    async fn sync_backend_models(&self, backend_id: Uuid, model_names: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query!(
            "DELETE FROM ollama_models WHERE backend_id = $1",
            backend_id,
        )
        .execute(&mut *tx)
        .await?;

        for name in model_names {
            sqlx::query!(
                r#"
                INSERT INTO ollama_models (model_name, backend_id, synced_at)
                VALUES ($1, $2, NOW())
                "#,
                name,
                backend_id,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            "SELECT DISTINCT model_name FROM ollama_models ORDER BY model_name ASC"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.model_name).collect())
    }

    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>> {
        let rows = sqlx::query!(
            r#"
            SELECT model_name, COUNT(backend_id) AS "backend_count!: i64"
            FROM ollama_models
            GROUP BY model_name
            ORDER BY model_name ASC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OllamaModelWithCount {
                model_name: r.model_name,
                backend_count: r.backend_count,
            })
            .collect())
    }

    async fn backends_for_model(&self, model_name: &str) -> Result<Vec<Uuid>> {
        let rows = sqlx::query!(
            "SELECT backend_id FROM ollama_models WHERE model_name = $1",
            model_name,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.backend_id).collect())
    }

    async fn backends_info_for_model(&self, model_name: &str) -> Result<Vec<OllamaBackendForModel>> {
        let rows = sqlx::query!(
            r#"
            SELECT om.backend_id AS "backend_id: Uuid", b.name, b.url, b.status
            FROM ollama_models om
            JOIN llm_backends b ON b.id = om.backend_id
            WHERE om.model_name = $1
            ORDER BY b.name ASC
            "#,
            model_name,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OllamaBackendForModel {
                backend_id: r.backend_id,
                name: r.name,
                url: r.url,
                status: r.status,
            })
            .collect())
    }

    async fn models_for_backend(&self, backend_id: Uuid) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            "SELECT model_name FROM ollama_models WHERE backend_id = $1 ORDER BY model_name ASC",
            backend_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.model_name).collect())
    }
}
