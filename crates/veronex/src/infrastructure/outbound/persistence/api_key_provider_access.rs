use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::application::ports::outbound::api_key_provider_access::ApiKeyProviderAccessRepository;

pub struct PostgresApiKeyProviderAccessRepository {
    pool: PgPool,
}

impl PostgresApiKeyProviderAccessRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ApiKeyProviderAccessRepository for PostgresApiKeyProviderAccessRepository {
    async fn list_allowed(&self, api_key_id: Uuid) -> anyhow::Result<Vec<Uuid>> {
        let rows = sqlx::query(
            "SELECT provider_id FROM api_key_provider_access WHERE api_key_id = $1 AND is_allowed = true"
        )
        .bind(api_key_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| r.try_get("provider_id").unwrap_or_default()).collect())
    }

    async fn set_access(&self, api_key_id: Uuid, provider_id: Uuid, allowed: bool) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO api_key_provider_access (api_key_id, provider_id, is_allowed)
             VALUES ($1, $2, $3)
             ON CONFLICT (api_key_id, provider_id) DO UPDATE SET is_allowed = $3"
        )
        .bind(api_key_id)
        .bind(provider_id)
        .bind(allowed)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list(&self, api_key_id: Uuid) -> anyhow::Result<Vec<(Uuid, bool)>> {
        let rows = sqlx::query(
            "SELECT provider_id, is_allowed FROM api_key_provider_access WHERE api_key_id = $1"
        )
        .bind(api_key_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| (
            r.try_get("provider_id").unwrap_or_default(),
            r.try_get("is_allowed").unwrap_or(true),
        )).collect())
    }

    async fn has_restrictions(&self, api_key_id: Uuid) -> anyhow::Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_key_provider_access WHERE api_key_id = $1"
        )
        .bind(api_key_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        Ok(count > 0)
    }
}
