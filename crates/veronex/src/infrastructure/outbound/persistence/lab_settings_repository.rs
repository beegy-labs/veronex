use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::lab_settings_repository::{
    LabSettings, LabSettingsRepository, LabSettingsUpdate,
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
            context_compression_enabled: bool,
            compression_model: Option<String>,
            context_budget_ratio: f32,
            compression_trigger_turns: i32,
            recent_verbatim_window: i32,
            compression_timeout_secs: i32,
            multiturn_min_params: i32,
            multiturn_min_ctx: i32,
            multiturn_allowed_models: Vec<String>,
            vision_model: Option<String>,
            handoff_enabled: bool,
            updated_at: chrono::DateTime<chrono::Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"SELECT
                gemini_function_calling,
                max_images_per_request,
                max_image_b64_bytes,
                context_compression_enabled,
                compression_model,
                context_budget_ratio,
                compression_trigger_turns,
                recent_verbatim_window,
                compression_timeout_secs,
                multiturn_min_params,
                multiturn_min_ctx,
                multiturn_allowed_models,
                vision_model,
                handoff_enabled,
                updated_at
               FROM lab_settings WHERE id = 1"#,
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed to get lab_settings")?;

        Ok(row
            .map(|r| LabSettings {
                gemini_function_calling: r.gemini_function_calling,
                max_images_per_request: r.max_images_per_request,
                max_image_b64_bytes: r.max_image_b64_bytes,
                context_compression_enabled: r.context_compression_enabled,
                compression_model: r.compression_model,
                context_budget_ratio: r.context_budget_ratio,
                compression_trigger_turns: r.compression_trigger_turns,
                recent_verbatim_window: r.recent_verbatim_window,
                compression_timeout_secs: r.compression_timeout_secs,
                multiturn_min_params: r.multiturn_min_params,
                multiturn_min_ctx: r.multiturn_min_ctx,
                multiturn_allowed_models: r.multiturn_allowed_models,
                vision_model: r.vision_model,
                handoff_enabled: r.handoff_enabled,
                updated_at: r.updated_at,
            })
            .unwrap_or_default())
    }

    async fn update(&self, patch: LabSettingsUpdate) -> Result<LabSettings> {
        // Nullable text fields (compression_model, vision_model) use a boolean flag + value
        // pair instead of COALESCE, so that Some(None) can explicitly clear the column.
        sqlx::query(
            r#"UPDATE lab_settings SET
                gemini_function_calling     = COALESCE($1,  gemini_function_calling),
                max_images_per_request      = COALESCE($2,  max_images_per_request),
                max_image_b64_bytes         = COALESCE($3,  max_image_b64_bytes),
                context_compression_enabled = COALESCE($4,  context_compression_enabled),
                compression_model           = CASE WHEN $5  THEN $6  ELSE compression_model END,
                context_budget_ratio        = COALESCE($7,  context_budget_ratio),
                compression_trigger_turns   = COALESCE($8,  compression_trigger_turns),
                recent_verbatim_window      = COALESCE($9,  recent_verbatim_window),
                compression_timeout_secs    = COALESCE($10, compression_timeout_secs),
                multiturn_min_params        = COALESCE($11, multiturn_min_params),
                multiturn_min_ctx           = COALESCE($12, multiturn_min_ctx),
                multiturn_allowed_models    = COALESCE($13, multiturn_allowed_models),
                vision_model                = CASE WHEN $14 THEN $15 ELSE vision_model END,
                handoff_enabled             = COALESCE($16, handoff_enabled),
                updated_at                  = now()
               WHERE id = 1"#,
        )
        .bind(patch.gemini_function_calling)
        .bind(patch.max_images_per_request)
        .bind(patch.max_image_b64_bytes)
        .bind(patch.context_compression_enabled)
        .bind(patch.compression_model.is_some())
        .bind(patch.compression_model.and_then(|v| v))
        .bind(patch.context_budget_ratio)
        .bind(patch.compression_trigger_turns)
        .bind(patch.recent_verbatim_window)
        .bind(patch.compression_timeout_secs)
        .bind(patch.multiturn_min_params)
        .bind(patch.multiturn_min_ctx)
        .bind(patch.multiturn_allowed_models)
        .bind(patch.vision_model.is_some())
        .bind(patch.vision_model.and_then(|v| v))
        .bind(patch.handoff_enabled)
        .execute(&self.pool)
        .await
        .context("failed to update lab_settings")?;

        self.get().await
    }
}
