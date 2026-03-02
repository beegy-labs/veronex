use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use crate::domain::entities::GeminiRateLimitPolicy;

pub struct PostgresGeminiPolicyRepository {
    pool: PgPool,
}

impl PostgresGeminiPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_policy(row: &sqlx::postgres::PgRow) -> Result<GeminiRateLimitPolicy> {
    use sqlx::Row as _;
    Ok(GeminiRateLimitPolicy {
        id: row.try_get("id").context("id")?,
        model_name: row.try_get("model_name").context("model_name")?,
        rpm_limit: row.try_get("rpm_limit").context("rpm_limit")?,
        rpd_limit: row.try_get("rpd_limit").context("rpd_limit")?,
        available_on_free_tier: row.try_get("available_on_free_tier").context("available_on_free_tier")?,
        updated_at: row.try_get::<DateTime<Utc>, _>("updated_at").context("updated_at")?,
    })
}

#[async_trait]
impl GeminiPolicyRepository for PostgresGeminiPolicyRepository {
    async fn list_all(&self) -> Result<Vec<GeminiRateLimitPolicy>> {
        let rows = sqlx::query(
            "SELECT id, model_name, rpm_limit, rpd_limit, available_on_free_tier, updated_at
             FROM gemini_rate_limit_policies
             ORDER BY model_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list gemini policies")?;

        rows.iter().map(row_to_policy).collect()
    }

    async fn get_for_model(&self, model_name: &str) -> Result<Option<GeminiRateLimitPolicy>> {
        // Try exact match first, then fall back to "*" global default.
        let row = sqlx::query(
            "SELECT id, model_name, rpm_limit, rpd_limit, available_on_free_tier, updated_at
             FROM gemini_rate_limit_policies
             WHERE model_name = $1 OR model_name = '*'
             ORDER BY CASE WHEN model_name = $1 THEN 0 ELSE 1 END
             LIMIT 1",
        )
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get gemini policy")?;

        match row {
            Some(r) => Ok(Some(row_to_policy(&r)?)),
            None => Ok(None),
        }
    }

    async fn upsert(&self, policy: &GeminiRateLimitPolicy) -> Result<()> {
        sqlx::query(
            "INSERT INTO gemini_rate_limit_policies
                 (id, model_name, rpm_limit, rpd_limit, available_on_free_tier, updated_at)
             VALUES ($1, $2, $3, $4, $5, now())
             ON CONFLICT (model_name) DO UPDATE
               SET rpm_limit = EXCLUDED.rpm_limit,
                   rpd_limit = EXCLUDED.rpd_limit,
                   available_on_free_tier = EXCLUDED.available_on_free_tier,
                   updated_at = now()",
        )
        .bind(policy.id)
        .bind(&policy.model_name)
        .bind(policy.rpm_limit)
        .bind(policy.rpd_limit)
        .bind(policy.available_on_free_tier)
        .execute(&self.pool)
        .await
        .context("failed to upsert gemini policy")?;

        Ok(())
    }
}
