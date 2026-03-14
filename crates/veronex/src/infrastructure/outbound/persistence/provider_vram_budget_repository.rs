use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::provider_vram_budget_repository::{
    ProviderVramBudget, ProviderVramBudgetRepository,
};

pub struct PostgresProviderVramBudgetRepository {
    pool: PgPool,
}

impl PostgresProviderVramBudgetRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct BudgetRow {
    safety_permil: i32,
    vram_total_source: String,
    kv_cache_type: String,
}

#[async_trait::async_trait]
impl ProviderVramBudgetRepository for PostgresProviderVramBudgetRepository {
    async fn get(&self, provider_id: Uuid) -> Result<Option<ProviderVramBudget>> {
        let row = sqlx::query_as::<_, BudgetRow>(
            "SELECT safety_permil, vram_total_source, kv_cache_type \
             FROM provider_vram_budget WHERE provider_id = $1",
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .context("get provider_vram_budget")?;

        Ok(row.map(|r| ProviderVramBudget {
            provider_id,
            safety_permil: r.safety_permil,
            vram_total_source: r.vram_total_source,
            kv_cache_type: r.kv_cache_type,
        }))
    }

    async fn upsert(&self, budget: &ProviderVramBudget) -> Result<()> {
        sqlx::query(
            "INSERT INTO provider_vram_budget \
                 (provider_id, safety_permil, vram_total_source, kv_cache_type, updated_at) \
             VALUES ($1, $2, $3, $4, now()) \
             ON CONFLICT (provider_id) DO UPDATE \
             SET safety_permil     = EXCLUDED.safety_permil, \
                 vram_total_source = EXCLUDED.vram_total_source, \
                 kv_cache_type     = EXCLUDED.kv_cache_type, \
                 updated_at        = now()",
        )
        .bind(budget.provider_id)
        .bind(budget.safety_permil)
        .bind(&budget.vram_total_source)
        .bind(&budget.kv_cache_type)
        .execute(&self.pool)
        .await
        .context("upsert provider_vram_budget")?;
        Ok(())
    }
}
