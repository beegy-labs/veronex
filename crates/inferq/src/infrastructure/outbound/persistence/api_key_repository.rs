use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::domain::entities::ApiKey;

/// PostgreSQL-backed implementation of `ApiKeyRepository`.
pub struct PostgresApiKeyRepository {
    pool: PgPool,
}

impl PostgresApiKeyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_api_key(row: &sqlx::postgres::PgRow) -> Result<ApiKey> {
    use sqlx::Row;

    Ok(ApiKey {
        id: row.try_get("id").context("missing column: id")?,
        key_hash: row.try_get("key_hash").context("missing column: key_hash")?,
        key_prefix: row
            .try_get("key_prefix")
            .context("missing column: key_prefix")?,
        tenant_id: row
            .try_get("tenant_id")
            .context("missing column: tenant_id")?,
        name: row.try_get("name").context("missing column: name")?,
        is_active: row
            .try_get("is_active")
            .context("missing column: is_active")?,
        rate_limit_rpm: row
            .try_get("rate_limit_rpm")
            .context("missing column: rate_limit_rpm")?,
        rate_limit_tpm: row
            .try_get("rate_limit_tpm")
            .context("missing column: rate_limit_tpm")?,
        expires_at: row
            .try_get("expires_at")
            .context("missing column: expires_at")?,
        created_at: row
            .try_get("created_at")
            .context("missing column: created_at")?,
        deleted_at: row
            .try_get("deleted_at")
            .context("missing column: deleted_at")?,
        key_type: row
            .try_get::<Option<String>, _>("key_type")
            .unwrap_or(None)
            .unwrap_or_else(|| "standard".to_string()),
        tier: row
            .try_get::<Option<String>, _>("tier")
            .unwrap_or(None)
            .unwrap_or_else(|| "paid".to_string()),
    })
}

#[async_trait]
impl ApiKeyRepository for PostgresApiKeyRepository {
    async fn create(&self, key: &ApiKey) -> Result<()> {
        sqlx::query(
            "INSERT INTO api_keys (id, key_hash, key_prefix, tenant_id, name, is_active, rate_limit_rpm, rate_limit_tpm, expires_at, created_at, key_type, tier)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(key.id)
        .bind(&key.key_hash)
        .bind(&key.key_prefix)
        .bind(&key.tenant_id)
        .bind(&key.name)
        .bind(key.is_active)
        .bind(key.rate_limit_rpm)
        .bind(key.rate_limit_tpm)
        .bind(key.expires_at)
        .bind(key.created_at)
        .bind(&key.key_type)
        .bind(&key.tier)
        .execute(&self.pool)
        .await
        .context("failed to create api key")?;

        Ok(())
    }

    async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let row = sqlx::query(
            "SELECT id, key_hash, key_prefix, tenant_id, name, is_active, rate_limit_rpm, rate_limit_tpm, expires_at, created_at, deleted_at, key_type, tier
             FROM api_keys WHERE key_hash = $1 AND deleted_at IS NULL",
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get api key by hash")?;

        match row {
            Some(r) => Ok(Some(row_to_api_key(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<ApiKey>> {
        let rows = sqlx::query(
            "SELECT id, key_hash, key_prefix, tenant_id, name, is_active, rate_limit_rpm, rate_limit_tpm, expires_at, created_at, deleted_at, key_type, tier
             FROM api_keys WHERE tenant_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .context("failed to list api keys by tenant")?;

        rows.iter().map(row_to_api_key).collect()
    }

    async fn revoke(&self, key_id: &Uuid) -> Result<()> {
        sqlx::query("UPDATE api_keys SET is_active = FALSE WHERE id = $1")
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("failed to revoke api key")?;

        Ok(())
    }

    async fn set_active(&self, key_id: &Uuid, active: bool) -> Result<()> {
        sqlx::query("UPDATE api_keys SET is_active = $1 WHERE id = $2 AND deleted_at IS NULL")
            .bind(active)
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("failed to set api key active state")?;

        Ok(())
    }

    async fn soft_delete(&self, key_id: &Uuid) -> Result<()> {
        sqlx::query("UPDATE api_keys SET deleted_at = NOW() WHERE id = $1")
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("failed to soft-delete api key")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn constructor_creates_struct() {
        fn _assert_trait_impl<T: ApiKeyRepository>() {}
        _assert_trait_impl::<PostgresApiKeyRepository>();
    }

    #[test]
    fn can_be_arc_dyn_trait() {
        fn _accepts_trait_object(_repo: Arc<dyn ApiKeyRepository>) {}
    }
}
