use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::backend_model_selection::{
    BackendModelSelectionRepository, BackendSelectedModel,
};

pub struct PostgresBackendModelSelectionRepository {
    pool: PgPool,
}

impl PostgresBackendModelSelectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BackendModelSelectionRepository for PostgresBackendModelSelectionRepository {
    async fn upsert_models(&self, backend_id: Uuid, models: &[String]) -> Result<()> {
        for model_name in models {
            sqlx::query!(
                r#"
                INSERT INTO backend_selected_models (backend_id, model_name, is_enabled, added_at)
                VALUES ($1, $2, true, NOW())
                ON CONFLICT (backend_id, model_name) DO NOTHING
                "#,
                backend_id,
                model_name,
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn list(&self, backend_id: Uuid) -> Result<Vec<BackendSelectedModel>> {
        let rows = sqlx::query!(
            r#"
            SELECT backend_id, model_name, is_enabled, added_at
            FROM backend_selected_models
            WHERE backend_id = $1
            ORDER BY model_name ASC
            "#,
            backend_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| BackendSelectedModel {
                backend_id: r.backend_id,
                model_name: r.model_name,
                is_enabled: r.is_enabled,
                added_at: r.added_at,
            })
            .collect())
    }

    async fn set_enabled(&self, backend_id: Uuid, model_name: &str, enabled: bool) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO backend_selected_models (backend_id, model_name, is_enabled, added_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (backend_id, model_name) DO UPDATE
                SET is_enabled = EXCLUDED.is_enabled
            "#,
            backend_id,
            model_name,
            enabled,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_enabled(&self, backend_id: Uuid) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            r#"
            SELECT model_name
            FROM backend_selected_models
            WHERE backend_id = $1 AND is_enabled = true
            ORDER BY model_name ASC
            "#,
            backend_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.model_name).collect())
    }
}
