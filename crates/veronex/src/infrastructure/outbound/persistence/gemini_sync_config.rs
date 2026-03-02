use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;

pub struct PostgresGeminiSyncConfigRepository {
    pool: PgPool,
}

impl PostgresGeminiSyncConfigRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GeminiSyncConfigRepository for PostgresGeminiSyncConfigRepository {
    async fn get_api_key(&self) -> Result<Option<String>> {
        let row = sqlx::query!(
            r#"SELECT api_key_encrypted FROM gemini_sync_config WHERE id = 1"#
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.api_key_encrypted))
    }

    async fn set_api_key(&self, api_key: &str) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO gemini_sync_config (id, api_key_encrypted, updated_at)
            VALUES (1, $1, NOW())
            ON CONFLICT (id) DO UPDATE
                SET api_key_encrypted = EXCLUDED.api_key_encrypted,
                    updated_at        = NOW()
            "#,
            api_key,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
