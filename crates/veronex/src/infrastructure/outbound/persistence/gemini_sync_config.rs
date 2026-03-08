use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;
use crate::domain::services::encryption::{decrypt_or_legacy, encrypt};

pub struct PostgresGeminiSyncConfigRepository {
    pool: PgPool,
    master_key: [u8; 32],
}

impl PostgresGeminiSyncConfigRepository {
    pub fn new(pool: PgPool, master_key: [u8; 32]) -> Self {
        Self { pool, master_key }
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

        match row {
            Some(r) => {
                let (plaintext, needs_re_encrypt) =
                    decrypt_or_legacy(&r.api_key_encrypted, &self.master_key);
                if needs_re_encrypt {
                    tracing::warn!("legacy plaintext gemini sync key detected — re-encrypting");
                    if let Err(e) = self.set_api_key(&plaintext).await {
                        tracing::error!("failed to re-encrypt legacy key: {e}");
                    }
                }
                Ok(Some(plaintext))
            }
            None => Ok(None),
        }
    }

    async fn set_api_key(&self, api_key: &str) -> Result<()> {
        let encrypted = encrypt(api_key, &self.master_key)?;
        sqlx::query!(
            r#"
            INSERT INTO gemini_sync_config (id, api_key_encrypted, updated_at)
            VALUES (1, $1, NOW())
            ON CONFLICT (id) DO UPDATE
                SET api_key_encrypted = EXCLUDED.api_key_encrypted,
                    updated_at        = NOW()
            "#,
            encrypted,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
