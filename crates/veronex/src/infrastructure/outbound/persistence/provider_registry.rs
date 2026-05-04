use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{LlmProviderStatus, ProviderType};
use crate::domain::services::encryption::{decrypt_or_legacy, encrypt};

/// Column list shared by all SELECT queries on llm_providers.
const PROVIDER_COLS: &str = "id, name, provider_type, url, api_key_encrypted, total_vram_mb, gpu_index, server_id, is_free_tier, num_parallel, status, registered_at";

pub struct PostgresProviderRegistry {
    pool: PgPool,
    master_key: [u8; 32],
}

impl PostgresProviderRegistry {
    pub fn new(pool: PgPool, master_key: [u8; 32]) -> Self {
        Self { pool, master_key }
    }
}

// ── Enum conversions ───────────────────────────────────────────────────────────

fn status_to_str(s: &LlmProviderStatus) -> &'static str {
    s.as_str()
}

fn str_to_status(s: &str) -> LlmProviderStatus {
    match s {
        "online" => LlmProviderStatus::Online,
        "degraded" => LlmProviderStatus::Degraded,
        _ => LlmProviderStatus::Offline,
    }
}

// ── Row mapping ────────────────────────────────────────────────────────────────

/// Returns `(provider, needs_re_encrypt)`.
fn row_to_provider(row: &sqlx::postgres::PgRow, master_key: &[u8; 32]) -> Result<(LlmProvider, bool)> {
    use sqlx::Row as _;

    let id: Uuid = row.try_get("id").context("id")?;
    let name: String = row.try_get("name").context("name")?;
    let provider_type_str: String = row.try_get("provider_type").context("provider_type")?;
    let url: String = row.try_get("url").context("url")?;
    let api_key_raw: Option<String> = row.try_get("api_key_encrypted").context("api_key_encrypted")?;
    let total_vram_mb: i64 = row.try_get("total_vram_mb").context("total_vram_mb")?;
    let gpu_index: Option<i16> = row.try_get("gpu_index").context("gpu_index")?;
    let server_id: Option<Uuid> = row.try_get("server_id").context("server_id")?;
    let is_free_tier: bool = row.try_get("is_free_tier").context("is_free_tier")?;
    let num_parallel: i16 = row.try_get("num_parallel").context("num_parallel")?;
    let status_str: String = row.try_get("status").context("status")?;
    let registered_at: DateTime<Utc> = row.try_get("registered_at").context("registered_at")?;

    // Decrypt API key at the persistence boundary; domain works with plaintext.
    let mut needs_re_encrypt = false;
    let api_key_encrypted = api_key_raw.map(|raw| {
        let (plaintext, re_encrypt) = decrypt_or_legacy(&raw, master_key);
        needs_re_encrypt = re_encrypt;
        plaintext
    });

    Ok((LlmProvider {
        id,
        name,
        provider_type: provider_type_str.parse::<ProviderType>().map_err(|e| anyhow::anyhow!(e))?,
        url,
        api_key_encrypted,
        total_vram_mb,
        gpu_index,
        server_id,
        is_free_tier,
        num_parallel,
        status: str_to_status(&status_str),
        registered_at,
    }, needs_re_encrypt))
}

// ── Re-encryption helper ──────────────────────────────────────────────────────

impl PostgresProviderRegistry {
    /// Re-encrypt a single provider's API key in place (UPDATE only the key column).
    async fn re_encrypt_key(&self, id: Uuid, plaintext: &str) {
        match encrypt(plaintext, &self.master_key) {
            Ok(encrypted) => {
                if let Err(e) = sqlx::query(
                    "UPDATE llm_providers SET api_key_encrypted = $1 WHERE id = $2",
                )
                .bind(&encrypted)
                .bind(id)
                .execute(&self.pool)
                .await
                {
                    tracing::error!(%id, "failed to re-encrypt legacy provider key: {e}");
                }
            }
            Err(e) => tracing::error!(%id, "encryption failed during re-encrypt: {e}"),
        }
    }
}

// ── Repository impl ────────────────────────────────────────────────────────────

#[async_trait]
impl LlmProviderRegistry for PostgresProviderRegistry {
    async fn register(&self, provider: &LlmProvider) -> Result<()> {
        let encrypted_key = provider
            .api_key_encrypted
            .as_deref()
            .map(|k| encrypt(k, &self.master_key))
            .transpose()
            .context("failed to encrypt provider api key")?;

        sqlx::query(
            "INSERT INTO llm_providers
                 (id, name, provider_type, url, api_key_encrypted,
                  total_vram_mb, gpu_index, server_id,
                  is_free_tier, num_parallel, status, registered_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(provider.id)
        .bind(&provider.name)
        .bind(provider.provider_type.as_str())
        .bind(&provider.url)
        .bind(&encrypted_key)
        .bind(provider.total_vram_mb)
        .bind(provider.gpu_index)
        .bind(provider.server_id)
        .bind(provider.is_free_tier)
        .bind(provider.num_parallel)
        .bind(status_to_str(&provider.status))
        .bind(provider.registered_at)
        .execute(&self.pool)
        .await
        .context("failed to register provider")?;

        Ok(())
    }

    async fn list_active(&self) -> Result<Vec<LlmProvider>> {
        let sql = format!("SELECT {PROVIDER_COLS} FROM llm_providers WHERE status = 'online' ORDER BY registered_at ASC LIMIT 10000");
        let rows = sqlx::query(&sql)
        .fetch_all(&self.pool)
        .await
        .context("failed to list active providers")?;

        let mut providers = Vec::with_capacity(rows.len());
        for r in &rows {
            let (p, needs) = row_to_provider(r, &self.master_key)?;
            if needs
                && let Some(ref key) = p.api_key_encrypted
            {
                tracing::warn!(id = %p.id, "legacy plaintext provider key — re-encrypting");
                self.re_encrypt_key(p.id, key).await;
            }
            providers.push(p);
        }
        Ok(providers)
    }

    async fn list_all(&self) -> Result<Vec<LlmProvider>> {
        let sql = format!("SELECT {PROVIDER_COLS} FROM llm_providers ORDER BY registered_at ASC LIMIT 10000");
        let rows = sqlx::query(&sql)
        .fetch_all(&self.pool)
        .await
        .context("failed to list all providers")?;

        let mut providers = Vec::with_capacity(rows.len());
        for r in &rows {
            let (p, needs) = row_to_provider(r, &self.master_key)?;
            if needs
                && let Some(ref key) = p.api_key_encrypted
            {
                tracing::warn!(id = %p.id, "legacy plaintext provider key — re-encrypting");
                self.re_encrypt_key(p.id, key).await;
            }
            providers.push(p);
        }
        Ok(providers)
    }

    async fn get(&self, id: Uuid) -> Result<Option<LlmProvider>> {
        let sql = format!("SELECT {PROVIDER_COLS} FROM llm_providers WHERE id = $1");
        let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get provider")?;

        match row {
            Some(r) => {
                let (provider, needs_re_encrypt) = row_to_provider(&r, &self.master_key)?;
                if needs_re_encrypt
                    && let Some(ref key) = provider.api_key_encrypted
                {
                    tracing::warn!(%id, "legacy plaintext provider key — re-encrypting");
                    self.re_encrypt_key(id, key).await;
                }
                Ok(Some(provider))
            }
            None => Ok(None),
        }
    }

    async fn update_status(&self, id: Uuid, status: LlmProviderStatus) -> Result<()> {
        sqlx::query("UPDATE llm_providers SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status_to_str(&status))
            .execute(&self.pool)
            .await
            .context("failed to update provider status")?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM llm_providers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to delete provider")?;

        Ok(())
    }

    async fn list_page(&self, search: &str, provider_type: Option<&str>, limit: i64, offset: i64) -> Result<(Vec<LlmProvider>, i64)> {
        let pattern = format!("%{}%", search);
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM llm_providers WHERE (name ILIKE $1 OR url ILIKE $1) AND ($2::text IS NULL OR provider_type = $2)"
        )
        .bind(&pattern)
        .bind(provider_type)
        .fetch_one(&self.pool)
        .await
        .context("failed to count providers")?;

        let sql = format!(
            "SELECT {PROVIDER_COLS} FROM llm_providers WHERE (name ILIKE $1 OR url ILIKE $1) AND ($2::text IS NULL OR provider_type = $2) ORDER BY registered_at ASC LIMIT $3 OFFSET $4"
        );
        let rows = sqlx::query(&sql)
            .bind(&pattern)
            .bind(provider_type)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .context("failed to list providers page")?;

        let mut providers = Vec::with_capacity(rows.len());
        for r in &rows {
            let (p, needs) = row_to_provider(r, &self.master_key)?;
            if needs && let Some(ref key) = p.api_key_encrypted {
                tracing::warn!(id = %p.id, "legacy plaintext provider key — re-encrypting");
                self.re_encrypt_key(p.id, key).await;
            }
            providers.push(p);
        }
        Ok((providers, total))
    }

    async fn update(&self, provider: &LlmProvider) -> Result<()> {
        let encrypted_key = provider
            .api_key_encrypted
            .as_deref()
            .map(|k| encrypt(k, &self.master_key))
            .transpose()
            .context("failed to encrypt provider api key")?;

        sqlx::query(
            "UPDATE llm_providers
             SET name = $1,
                 url = $2,
                 api_key_encrypted = COALESCE($3, api_key_encrypted),
                 total_vram_mb = $4,
                 gpu_index = $5,
                 server_id = $6,
                 is_free_tier = $7,
                 num_parallel = $8
             WHERE id = $9",
        )
        .bind(&provider.name)
        .bind(&provider.url)
        .bind(&encrypted_key)
        .bind(provider.total_vram_mb)
        .bind(provider.gpu_index)
        .bind(provider.server_id)
        .bind(provider.is_free_tier)
        .bind(provider.num_parallel)
        .bind(provider.id)
        .execute(&self.pool)
        .await
        .context("failed to update provider")?;

        Ok(())
    }
}
