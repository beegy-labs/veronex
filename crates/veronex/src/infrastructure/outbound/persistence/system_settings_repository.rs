//! Postgres implementation of [`SystemSettingsRepository`] (Phase 3).

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::system_settings_repository::{
    SystemSetting, SystemSettingsRepository,
};

const SS_COLS: &str = "key, value, description, updated_at, updated_by";

pub struct PostgresSystemSettingsRepository {
    pool: PgPool,
}

impl PostgresSystemSettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_setting(row: &sqlx::postgres::PgRow) -> Result<SystemSetting> {
    use sqlx::Row as _;
    Ok(SystemSetting {
        key: row.try_get("key").context("key")?,
        value: row.try_get("value").context("value")?,
        description: row.try_get("description").context("description")?,
        updated_at: row
            .try_get::<DateTime<Utc>, _>("updated_at")
            .context("updated_at")?,
        updated_by: row
            .try_get::<Option<Uuid>, _>("updated_by")
            .context("updated_by")?,
    })
}

#[async_trait]
impl SystemSettingsRepository for PostgresSystemSettingsRepository {
    async fn get(&self, key: &str) -> Result<Option<SystemSetting>> {
        let q = format!("SELECT {SS_COLS} FROM system_settings WHERE key = $1");
        let row = sqlx::query(&q).bind(key).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_setting).transpose()
    }

    async fn list_all(&self) -> Result<Vec<SystemSetting>> {
        let q = format!("SELECT {SS_COLS} FROM system_settings ORDER BY key");
        let rows = sqlx::query(&q).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_setting).collect()
    }

    async fn upsert(
        &self,
        key: &str,
        value: &str,
        description: Option<&str>,
        by: Option<Uuid>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO system_settings (key, value, description, updated_at, updated_by)
             VALUES ($1, $2, $3, now(), $4)
             ON CONFLICT (key) DO UPDATE SET
                 value       = EXCLUDED.value,
                 description = COALESCE(EXCLUDED.description, system_settings.description),
                 updated_at  = now(),
                 updated_by  = EXCLUDED.updated_by",
        )
        .bind(key)
        .bind(value)
        .bind(description)
        .bind(by)
        .execute(&self.pool)
        .await
        .with_context(|| format!("upsert system_settings[{key}]"))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let r = sqlx::query("DELETE FROM system_settings WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}
