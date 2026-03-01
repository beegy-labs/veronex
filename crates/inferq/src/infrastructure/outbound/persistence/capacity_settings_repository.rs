use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::capacity_settings_repository::{
    CapacitySettings, CapacitySettingsRepository,
};

pub struct PostgresCapacitySettingsRepository {
    pool: PgPool,
}

impl PostgresCapacitySettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CapacitySettingsRepository for PostgresCapacitySettingsRepository {
    async fn get(&self) -> Result<CapacitySettings> {
        #[derive(sqlx::FromRow)]
        struct Row {
            analyzer_model:      String,
            batch_enabled:       bool,
            batch_interval_secs: i32,
            last_run_at:         Option<chrono::DateTime<chrono::Utc>>,
            last_run_status:     Option<String>,
        }

        let row = sqlx::query_as::<_, Row>(
            "SELECT analyzer_model, batch_enabled, batch_interval_secs, last_run_at, last_run_status
             FROM capacity_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed to get capacity_settings")?;

        Ok(row
            .map(|r| CapacitySettings {
                analyzer_model:      r.analyzer_model,
                batch_enabled:       r.batch_enabled,
                batch_interval_secs: r.batch_interval_secs,
                last_run_at:         r.last_run_at,
                last_run_status:     r.last_run_status,
            })
            .unwrap_or_default())
    }

    async fn update_settings(
        &self,
        model: Option<&str>,
        batch_enabled: Option<bool>,
        interval_secs: Option<i32>,
    ) -> Result<CapacitySettings> {
        sqlx::query(
            "UPDATE capacity_settings SET
                 analyzer_model      = COALESCE($1, analyzer_model),
                 batch_enabled       = COALESCE($2, batch_enabled),
                 batch_interval_secs = COALESCE($3, batch_interval_secs),
                 updated_at          = now()
             WHERE id = 1",
        )
        .bind(model)
        .bind(batch_enabled)
        .bind(interval_secs)
        .execute(&self.pool)
        .await
        .context("failed to update capacity_settings")?;

        self.get().await
    }

    async fn record_run(&self, status: &str) -> Result<()> {
        sqlx::query(
            "UPDATE capacity_settings SET last_run_at = now(), last_run_status = $1, updated_at = now() WHERE id = 1",
        )
        .bind(status)
        .execute(&self.pool)
        .await
        .context("failed to record capacity analysis run")?;

        Ok(())
    }
}
