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
            analyzer_model:     String,
            sync_enabled:       bool,
            sync_interval_secs: i32,
            probe_permits:      i32,
            probe_rate:         i32,
            last_run_at:        Option<chrono::DateTime<chrono::Utc>>,
            last_run_status:    Option<String>,
        }

        let row = sqlx::query_as::<_, Row>(
            "SELECT analyzer_model, sync_enabled, sync_interval_secs,
                    probe_permits, probe_rate, last_run_at, last_run_status
             FROM capacity_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed to get capacity_settings")?;

        Ok(row
            .map(|r| CapacitySettings {
                analyzer_model:     r.analyzer_model,
                sync_enabled:       r.sync_enabled,
                sync_interval_secs: r.sync_interval_secs,
                probe_permits:      r.probe_permits,
                probe_rate:         r.probe_rate,
                last_run_at:        r.last_run_at,
                last_run_status:    r.last_run_status,
            })
            .unwrap_or_default())
    }

    async fn update_settings(
        &self,
        model: Option<&str>,
        sync_enabled: Option<bool>,
        interval_secs: Option<i32>,
        probe_permits: Option<i32>,
        probe_rate: Option<i32>,
    ) -> Result<CapacitySettings> {
        sqlx::query(
            "INSERT INTO capacity_settings (id) VALUES (1)
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .context("failed to ensure capacity_settings row")?;

        sqlx::query(
            "UPDATE capacity_settings SET
                 analyzer_model     = COALESCE($1, analyzer_model),
                 sync_enabled       = COALESCE($2, sync_enabled),
                 sync_interval_secs = COALESCE($3, sync_interval_secs),
                 probe_permits      = COALESCE($4, probe_permits),
                 probe_rate         = COALESCE($5, probe_rate),
                 updated_at         = now()
             WHERE id = 1",
        )
        .bind(model)
        .bind(sync_enabled)
        .bind(interval_secs)
        .bind(probe_permits)
        .bind(probe_rate)
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
        .context("failed to record sync run")?;

        Ok(())
    }
}
