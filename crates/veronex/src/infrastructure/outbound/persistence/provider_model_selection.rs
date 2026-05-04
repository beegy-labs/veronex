use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::provider_model_selection::{
    ProviderModelSelectionRepository, ProviderSelectedModel,
};

pub struct PostgresProviderModelSelectionRepository {
    pool: PgPool,
}

impl PostgresProviderModelSelectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProviderModelSelectionRepository for PostgresProviderModelSelectionRepository {
    async fn upsert_models(&self, provider_id: Uuid, models: &[String]) -> Result<()> {
        if models.is_empty() {
            return Ok(());
        }
        // Single UNNEST batch insert — avoids N separate round-trips.
        sqlx::query(
            "INSERT INTO provider_selected_models (provider_id, model_name, is_enabled, added_at)
             SELECT $1, UNNEST($2::text[]), true, NOW()
             ON CONFLICT (provider_id, model_name) DO NOTHING",
        )
        .bind(provider_id)
        .bind(models)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list(&self, provider_id: Uuid) -> Result<Vec<ProviderSelectedModel>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            provider_id: Uuid,
            model_name: String,
            is_enabled: bool,
            added_at: chrono::DateTime<chrono::Utc>,
        }

        let rows: Vec<Row> = sqlx::query_as(
            "SELECT provider_id, model_name, is_enabled, added_at
             FROM provider_selected_models
             WHERE provider_id = $1
             ORDER BY model_name ASC
             LIMIT 10000",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ProviderSelectedModel {
                provider_id: r.provider_id,
                model_name: r.model_name,
                is_enabled: r.is_enabled,
                added_at: r.added_at,
            })
            .collect())
    }

    async fn set_enabled(&self, provider_id: Uuid, model_name: &str, enabled: bool) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO provider_selected_models (provider_id, model_name, is_enabled, added_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (provider_id, model_name) DO UPDATE
                SET is_enabled = EXCLUDED.is_enabled
            "#,
            provider_id,
            model_name,
            enabled,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_enabled(&self, provider_id: Uuid) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            r#"
            SELECT model_name
            FROM provider_selected_models
            WHERE provider_id = $1 AND is_enabled = true
            ORDER BY model_name ASC
            "#,
            provider_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.model_name).collect())
    }

    async fn list_disabled(&self, provider_id: Uuid) -> Result<Vec<String>> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT model_name FROM provider_selected_models \
             WHERE provider_id = $1 AND is_enabled = false \
             ORDER BY model_name ASC",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}
