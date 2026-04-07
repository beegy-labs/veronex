use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::application::ports::outbound::global_model_settings::{
    GlobalModelSetting, GlobalModelSettingsRepository,
};

pub struct PostgresGlobalModelSettingsRepository {
    pool: PgPool,
}

impl PostgresGlobalModelSettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GlobalModelSettingsRepository for PostgresGlobalModelSettingsRepository {
    async fn list(&self) -> anyhow::Result<Vec<GlobalModelSetting>> {
        let rows = sqlx::query(
            "SELECT model_name, is_enabled FROM global_model_settings ORDER BY model_name LIMIT 10000"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| GlobalModelSetting {
            model_name: r.try_get("model_name").unwrap_or_default(),
            is_enabled: r.try_get("is_enabled").unwrap_or(true),
        }).collect())
    }

    async fn is_enabled(&self, model_name: &str) -> anyhow::Result<bool> {
        let row = sqlx::query(
            "SELECT is_enabled FROM global_model_settings WHERE model_name = $1"
        )
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.try_get("is_enabled").unwrap_or(true)).unwrap_or(true))
    }

    async fn set_enabled(&self, model_name: &str, enabled: bool) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO global_model_settings (model_name, is_enabled, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (model_name) DO UPDATE SET is_enabled = $2, updated_at = NOW()"
        )
        .bind(model_name)
        .bind(enabled)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_disabled(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT model_name FROM global_model_settings WHERE is_enabled = false ORDER BY model_name LIMIT 10000"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| r.try_get("model_name").unwrap_or_default()).collect())
    }
}
