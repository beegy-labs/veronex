use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::ollama_model_repository::{
    OllamaProviderForModel, OllamaModelRepository, OllamaModelWithCount,
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
    async fn sync_provider_models(&self, provider_id: Uuid, model_names: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query!(
            "DELETE FROM ollama_models WHERE provider_id = $1",
            provider_id,
        )
        .execute(&mut *tx)
        .await?;

        for name in model_names {
            sqlx::query!(
                r#"
                INSERT INTO ollama_models (model_name, provider_id, synced_at)
                VALUES ($1, $2, NOW())
                "#,
                name,
                provider_id,
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
            SELECT model_name, COUNT(provider_id) AS "provider_count!: i64"
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
                provider_count: r.provider_count,
            })
            .collect())
    }

    async fn providers_for_model(&self, model_name: &str) -> Result<Vec<Uuid>> {
        let rows = sqlx::query!(
            "SELECT provider_id FROM ollama_models WHERE model_name = $1",
            model_name,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.provider_id).collect())
    }

    async fn providers_info_for_model(&self, model_name: &str) -> Result<Vec<OllamaProviderForModel>> {
        let rows = sqlx::query!(
            r#"
            SELECT om.provider_id AS "provider_id: Uuid", b.name, b.url, b.status
            FROM ollama_models om
            JOIN llm_providers b ON b.id = om.provider_id
            WHERE om.model_name = $1
            ORDER BY b.name ASC
            "#,
            model_name,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OllamaProviderForModel {
                provider_id: r.provider_id,
                name: r.name,
                url: r.url,
                status: r.status,
            })
            .collect())
    }

    async fn models_for_provider(&self, provider_id: Uuid) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            "SELECT model_name FROM ollama_models WHERE provider_id = $1 ORDER BY model_name ASC",
            provider_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.model_name).collect())
    }
}
