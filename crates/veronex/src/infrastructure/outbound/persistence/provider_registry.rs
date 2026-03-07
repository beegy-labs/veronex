use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{LlmProviderStatus, ProviderType};

/// Column list shared by all SELECT queries on llm_providers.
const PROVIDER_COLS: &str = "id, name, provider_type, url, api_key_encrypted, is_active, total_vram_mb, gpu_index, server_id, agent_url, is_free_tier, status, registered_at";

pub struct PostgresProviderRegistry {
    pool: PgPool,
}

impl PostgresProviderRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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

fn row_to_provider(row: &sqlx::postgres::PgRow) -> Result<LlmProvider> {
    use sqlx::Row as _;

    let id: Uuid = row.try_get("id").context("id")?;
    let name: String = row.try_get("name").context("name")?;
    let provider_type_str: String = row.try_get("provider_type").context("provider_type")?;
    let url: String = row.try_get("url").context("url")?;
    let api_key_encrypted: Option<String> = row.try_get("api_key_encrypted").context("api_key_encrypted")?;
    let is_active: bool = row.try_get("is_active").context("is_active")?;
    let total_vram_mb: i64 = row.try_get("total_vram_mb").context("total_vram_mb")?;
    let gpu_index: Option<i16> = row.try_get("gpu_index").context("gpu_index")?;
    let server_id: Option<Uuid> = row.try_get("server_id").context("server_id")?;
    let agent_url: Option<String> = row.try_get("agent_url").context("agent_url")?;
    let is_free_tier: bool = row.try_get("is_free_tier").context("is_free_tier")?;
    let status_str: String = row.try_get("status").context("status")?;
    let registered_at: DateTime<Utc> = row.try_get("registered_at").context("registered_at")?;

    Ok(LlmProvider {
        id,
        name,
        provider_type: provider_type_str.parse::<ProviderType>().map_err(|e| anyhow::anyhow!(e))?,
        url,
        api_key_encrypted,
        is_active,
        total_vram_mb,
        gpu_index,
        server_id,
        agent_url,
        is_free_tier,
        status: str_to_status(&status_str),
        registered_at,
    })
}

// ── Repository impl ────────────────────────────────────────────────────────────

#[async_trait]
impl LlmProviderRegistry for PostgresProviderRegistry {
    async fn register(&self, provider: &LlmProvider) -> Result<()> {
        sqlx::query(
            "INSERT INTO llm_providers
                 (id, name, provider_type, url, api_key_encrypted, is_active,
                  total_vram_mb, gpu_index, server_id, agent_url,
                  is_free_tier, status, registered_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(provider.id)
        .bind(&provider.name)
        .bind(provider.provider_type.as_str())
        .bind(&provider.url)
        .bind(&provider.api_key_encrypted)
        .bind(provider.is_active)
        .bind(provider.total_vram_mb)
        .bind(provider.gpu_index)
        .bind(provider.server_id)
        .bind(&provider.agent_url)
        .bind(provider.is_free_tier)
        .bind(status_to_str(&provider.status))
        .bind(provider.registered_at)
        .execute(&self.pool)
        .await
        .context("failed to register provider")?;

        Ok(())
    }

    async fn list_active(&self) -> Result<Vec<LlmProvider>> {
        let sql = format!("SELECT {PROVIDER_COLS} FROM llm_providers WHERE is_active = true AND status = 'online' ORDER BY registered_at ASC");
        let rows = sqlx::query(&sql)
        .fetch_all(&self.pool)
        .await
        .context("failed to list active providers")?;

        rows.iter().map(row_to_provider).collect()
    }

    async fn list_all(&self) -> Result<Vec<LlmProvider>> {
        let sql = format!("SELECT {PROVIDER_COLS} FROM llm_providers ORDER BY registered_at ASC");
        let rows = sqlx::query(&sql)
        .fetch_all(&self.pool)
        .await
        .context("failed to list all providers")?;

        rows.iter().map(row_to_provider).collect()
    }

    async fn get(&self, id: Uuid) -> Result<Option<LlmProvider>> {
        let sql = format!("SELECT {PROVIDER_COLS} FROM llm_providers WHERE id = $1");
        let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get provider")?;

        match row {
            Some(r) => Ok(Some(row_to_provider(&r)?)),
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

    async fn deactivate(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE llm_providers SET is_active = false WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to deactivate provider")?;

        Ok(())
    }

    async fn update(&self, provider: &LlmProvider) -> Result<()> {
        sqlx::query(
            "UPDATE llm_providers
             SET name = $1,
                 url = $2,
                 api_key_encrypted = COALESCE($3, api_key_encrypted),
                 total_vram_mb = $4,
                 gpu_index = $5,
                 server_id = $6,
                 agent_url = $7,
                 is_free_tier = $8,
                 is_active = $9
             WHERE id = $10",
        )
        .bind(&provider.name)
        .bind(&provider.url)
        .bind(&provider.api_key_encrypted)
        .bind(provider.total_vram_mb)
        .bind(provider.gpu_index)
        .bind(provider.server_id)
        .bind(&provider.agent_url)
        .bind(provider.is_free_tier)
        .bind(provider.is_active)
        .bind(provider.id)
        .execute(&self.pool)
        .await
        .context("failed to update provider")?;

        Ok(())
    }
}
