use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::gemini_repository::{
    GeminiModel, GeminiModelRepository,
};

pub struct PostgresGeminiModelRepository {
    pool: PgPool,
}

impl PostgresGeminiModelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GeminiModelRepository for PostgresGeminiModelRepository {
    async fn sync_models(&self, model_names: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM gemini_models")
            .execute(&mut *tx)
            .await?;

        if !model_names.is_empty() {
            // Single UNNEST batch insert — avoids N separate round-trips.
            sqlx::query(
                "INSERT INTO gemini_models (model_name, synced_at)
                 SELECT UNNEST($1::text[]), NOW()",
            )
            .bind(model_names)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<GeminiModel>> {
        #[derive(sqlx::FromRow)]
        struct Row { model_name: String, synced_at: chrono::DateTime<chrono::Utc> }

        let rows: Vec<Row> = sqlx::query_as(
            "SELECT model_name, synced_at FROM gemini_models ORDER BY model_name ASC LIMIT 1000"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| GeminiModel {
                model_name: r.model_name,
                synced_at: r.synced_at,
            })
            .collect())
    }
}
