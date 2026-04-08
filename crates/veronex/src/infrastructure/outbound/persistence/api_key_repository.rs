use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use super::{parse_db_enum, SOFT_DELETE};
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::domain::entities::ApiKey;
use crate::domain::enums::KeyTier;

/// Column list shared by all SELECT queries on api_keys.
const API_KEY_COLS: &str = "id, key_hash, key_prefix, tenant_id, name, is_active, rate_limit_rpm, rate_limit_tpm, expires_at, created_at, deleted_at, key_type, tier, mcp_cap_points, account_id";

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
        key_type: parse_db_enum(row.try_get("key_type").unwrap_or(None), "key_type"),
        tier: parse_db_enum(row.try_get("tier").unwrap_or(None), "tier"),
        mcp_cap_points: row.try_get("mcp_cap_points").unwrap_or(3),
        account_id: row.try_get("account_id").unwrap_or(None),
    })
}

#[async_trait]
impl ApiKeyRepository for PostgresApiKeyRepository {
    async fn create(&self, key: &ApiKey) -> Result<()> {
        sqlx::query(
            "INSERT INTO api_keys (id, key_hash, key_prefix, tenant_id, name, is_active, rate_limit_rpm, rate_limit_tpm, expires_at, created_at, key_type, tier, account_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
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
        .bind(key.key_type.as_str())
        .bind(key.tier.as_str())
        .bind(key.account_id)
        .execute(&self.pool)
        .await
        .context("failed to create api key")?;

        Ok(())
    }

    async fn get_by_id(&self, key_id: &Uuid) -> Result<Option<ApiKey>> {
        let sql = format!("SELECT {API_KEY_COLS} FROM api_keys WHERE id = $1 AND {SOFT_DELETE}");
        let row = sqlx::query(&sql)
            .bind(key_id)
            .fetch_optional(&self.pool)
            .await
            .context("failed to get api key by id")?;

        match row {
            Some(r) => Ok(Some(row_to_api_key(&r)?)),
            None => Ok(None),
        }
    }

    async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let sql = format!("SELECT {API_KEY_COLS} FROM api_keys WHERE key_hash = $1 AND {SOFT_DELETE}");
        let row = sqlx::query(&sql)
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
        let sql = format!("SELECT {API_KEY_COLS} FROM api_keys WHERE tenant_id = $1 AND {SOFT_DELETE} ORDER BY created_at DESC LIMIT 10000");
        let rows = sqlx::query(&sql)
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .context("failed to list api keys by tenant")?;

        rows.iter().map(row_to_api_key).collect()
    }

    async fn list_all(&self) -> Result<Vec<ApiKey>> {
        let sql = format!("SELECT {API_KEY_COLS} FROM api_keys WHERE {SOFT_DELETE} ORDER BY created_at DESC LIMIT 10000");
        let rows = sqlx::query(&sql)
        .fetch_all(&self.pool)
        .await
        .context("failed to list all api keys")?;

        rows.iter().map(row_to_api_key).collect()
    }

    async fn list_page(&self, search: &str, limit: i64, offset: i64) -> Result<(Vec<ApiKey>, i64)> {
        let pattern = format!("%{}%", search);
        let total: i64 = sqlx::query_scalar(
            &format!("SELECT COUNT(*) FROM api_keys WHERE {SOFT_DELETE} AND name ILIKE $1")
        )
        .bind(&pattern)
        .fetch_one(&self.pool)
        .await
        .context("failed to count api keys")?;

        let sql = format!(
            "SELECT {API_KEY_COLS} FROM api_keys WHERE {SOFT_DELETE} AND name ILIKE $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        );
        let rows = sqlx::query(&sql)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .context("failed to list api keys page")?;

        rows.iter().map(row_to_api_key).collect::<Result<Vec<_>>>().map(|v| (v, total))
    }

    async fn list_by_tenant_page(&self, tenant_id: &str, search: &str, limit: i64, offset: i64) -> Result<(Vec<ApiKey>, i64)> {
        let pattern = format!("%{}%", search);
        let total: i64 = sqlx::query_scalar(
            &format!("SELECT COUNT(*) FROM api_keys WHERE tenant_id = $1 AND {SOFT_DELETE} AND name ILIKE $2")
        )
        .bind(tenant_id)
        .bind(&pattern)
        .fetch_one(&self.pool)
        .await
        .context("failed to count tenant api keys")?;

        let sql = format!(
            "SELECT {API_KEY_COLS} FROM api_keys WHERE tenant_id = $1 AND {SOFT_DELETE} AND name ILIKE $2 ORDER BY created_at DESC LIMIT $3 OFFSET $4"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .context("failed to list tenant api keys page")?;

        rows.iter().map(row_to_api_key).collect::<Result<Vec<_>>>().map(|v| (v, total))
    }

    async fn revoke(&self, key_id: &Uuid) -> Result<()> {
        sqlx::query(&format!("UPDATE api_keys SET is_active = FALSE WHERE id = $1 AND {SOFT_DELETE}"))
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("failed to revoke api key")?;

        Ok(())
    }

    async fn set_active(&self, key_id: &Uuid, active: bool) -> Result<()> {
        sqlx::query(&format!("UPDATE api_keys SET is_active = $1 WHERE id = $2 AND {SOFT_DELETE}"))
            .bind(active)
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("failed to set api key active state")?;

        Ok(())
    }

    async fn set_tier(&self, key_id: &Uuid, tier: &KeyTier) -> Result<()> {
        sqlx::query(&format!("UPDATE api_keys SET tier = $1 WHERE id = $2 AND {SOFT_DELETE}"))
            .bind(tier.as_str())
            .bind(key_id)
            .execute(&self.pool)
            .await
            .context("failed to set api key tier")?;

        Ok(())
    }

    async fn update_fields(&self, key_id: &Uuid, is_active: Option<bool>, tier: Option<&KeyTier>) -> Result<()> {
        sqlx::query(
            &format!("UPDATE api_keys SET is_active = COALESCE($1, is_active), tier = COALESCE($2, tier) WHERE id = $3 AND {SOFT_DELETE}"),
        )
        .bind(is_active)
        .bind(tier.map(|t| t.as_str()))
        .bind(key_id)
        .execute(&self.pool)
        .await
        .context("failed to update api key fields")?;
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

    async fn regenerate(&self, key_id: &Uuid, new_hash: &str, new_prefix: &str) -> Result<()> {
        sqlx::query(&format!(
            "UPDATE api_keys SET key_hash = $1, key_prefix = $2 WHERE id = $3 AND {SOFT_DELETE}"
        ))
        .bind(new_hash)
        .bind(new_prefix)
        .bind(key_id)
        .execute(&self.pool)
        .await
        .context("failed to regenerate api key")?;
        Ok(())
    }

    async fn soft_delete_by_tenant(&self, tenant_id: &str) -> Result<u64> {
        let result = sqlx::query(
            &format!("UPDATE api_keys SET deleted_at = NOW() WHERE tenant_id = $1 AND {SOFT_DELETE}"),
        )
        .bind(tenant_id)
        .execute(&self.pool)
        .await
        .context("failed to soft-delete api keys by tenant")?;

        Ok(result.rows_affected())
    }
}
