use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::lab_settings_repository::{
    LabSettings, LabSettingsRepository,
};

pub struct PostgresLabSettingsRepository {
    pool: PgPool,
}

impl PostgresLabSettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LabSettingsRepository for PostgresLabSettingsRepository {
    async fn get(&self) -> Result<LabSettings> {
        #[derive(sqlx::FromRow)]
        struct Row {
            gemini_function_calling: bool,
            max_images_per_request: i32,
            max_image_b64_bytes: i32,
            updated_at: chrono::DateTime<chrono::Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            "SELECT gemini_function_calling, max_images_per_request, max_image_b64_bytes, updated_at FROM lab_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed to get lab_settings")?;

        Ok(row
            .map(|r| LabSettings {
                gemini_function_calling: r.gemini_function_calling,
                max_images_per_request: r.max_images_per_request,
                max_image_b64_bytes: r.max_image_b64_bytes,
                updated_at: r.updated_at,
            })
            .unwrap_or_default())
    }

    async fn update(
        &self,
        gemini_function_calling: Option<bool>,
        max_images_per_request: Option<i32>,
        max_image_b64_bytes: Option<i32>,
    ) -> Result<LabSettings> {
        sqlx::query(
            "UPDATE lab_settings SET
                 gemini_function_calling = COALESCE($1, gemini_function_calling),
                 max_images_per_request  = COALESCE($2, max_images_per_request),
                 max_image_b64_bytes     = COALESCE($3, max_image_b64_bytes),
                 updated_at              = now()
             WHERE id = 1",
        )
        .bind(gemini_function_calling)
        .bind(max_images_per_request)
        .bind(max_image_b64_bytes)
        .execute(&self.pool)
        .await
        .context("failed to update lab_settings")?;

        self.get().await
    }
}
