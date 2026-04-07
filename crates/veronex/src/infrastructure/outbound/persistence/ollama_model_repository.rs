use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::ollama_model_repository::{
    ModelPage, OllamaProviderForModel, OllamaModelRepository, OllamaModelWithCount, ProviderPage,
};

pub struct PostgresOllamaModelRepository {
    pool: PgPool,
}

impl PostgresOllamaModelRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OllamaModelRepository for PostgresOllamaModelRepository {
    async fn sync_provider_models(&self, provider_id: Uuid, model_names: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query!(
            "DELETE FROM ollama_models WHERE provider_id = $1",
            provider_id,
        )
        .execute(&mut *tx)
        .await?;

        for name in model_names {
            sqlx::query!(
                r#"
                INSERT INTO ollama_models (model_name, provider_id, synced_at)
                VALUES ($1, $2, NOW())
                "#,
                name,
                provider_id,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<String>> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT model_name FROM ollama_models ORDER BY model_name ASC LIMIT 10000"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>> {
        #[derive(sqlx::FromRow)]
        struct Row { model_name: String, provider_count: i64, max_ctx: Option<i32> }

        let rows: Vec<Row> = sqlx::query_as(
            r#"
            SELECT om.model_name,
                   COUNT(om.provider_id) AS provider_count,
                   MAX(mvp.max_ctx)      AS max_ctx
            FROM ollama_models om
            LEFT JOIN model_vram_profiles mvp
                   ON mvp.provider_id = om.provider_id
                  AND mvp.model_name  = om.model_name
            GROUP BY om.model_name
            ORDER BY om.model_name ASC
            LIMIT 10000
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| OllamaModelWithCount {
            model_name: r.model_name,
            provider_count: r.provider_count,
            max_ctx: r.max_ctx.unwrap_or(0),
        }).collect())
    }

    async fn list_with_counts_page(&self, search: &str, limit: i64, offset: i64) -> Result<ModelPage> {
        let pattern = format!("%{}%", search);
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT model_name) AS "n!: i64" FROM ollama_models WHERE model_name ILIKE $1"#,
            pattern,
        )
        .fetch_one(&self.pool)
        .await?;

        #[derive(sqlx::FromRow)]
        struct Row {
            model_name: String,
            provider_count: i64,
            max_ctx: Option<i32>,
        }

        let rows: Vec<Row> = sqlx::query_as(
            r#"
            SELECT om.model_name,
                   COUNT(om.provider_id)          AS provider_count,
                   MAX(mvp.max_ctx)               AS max_ctx
            FROM ollama_models om
            LEFT JOIN model_vram_profiles mvp
                   ON mvp.provider_id = om.provider_id
                  AND mvp.model_name  = om.model_name
            WHERE om.model_name ILIKE $1
            GROUP BY om.model_name
            ORDER BY om.model_name ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(ModelPage {
            items: rows.into_iter().map(|r| OllamaModelWithCount {
                model_name: r.model_name,
                provider_count: r.provider_count,
                max_ctx: r.max_ctx.unwrap_or(0),
            }).collect(),
            total,
        })
    }

    async fn providers_for_model(&self, model_name: &str) -> Result<Vec<Uuid>> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT provider_id FROM ollama_models WHERE model_name = $1 LIMIT 10000",
        )
        .bind(model_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn providers_info_for_model_page(
        &self,
        model_name: &str,
        search: &str,
        limit: i64,
        offset: i64,
    ) -> Result<ProviderPage> {
        let pattern = format!("%{}%", search);
        let total = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "n!: i64"
            FROM ollama_models om
            JOIN llm_providers b ON b.id = om.provider_id
            WHERE om.model_name = $1
              AND (b.name ILIKE $2 OR b.url ILIKE $2)
            "#,
            model_name,
            pattern,
        )
        .fetch_one(&self.pool)
        .await?;

        let rows: Vec<_> = sqlx::query!(
            r#"
            SELECT
                om.provider_id AS "provider_id: Uuid",
                b.name,
                b.url,
                b.status,
                COALESCE(pms.is_enabled, true) AS "is_enabled!: bool"
            FROM ollama_models om
            JOIN llm_providers b ON b.id = om.provider_id
            LEFT JOIN provider_selected_models pms
                ON pms.provider_id = om.provider_id AND pms.model_name = om.model_name
            WHERE om.model_name = $1
              AND (b.name ILIKE $2 OR b.url ILIKE $2)
            ORDER BY b.name ASC
            LIMIT $3 OFFSET $4
            "#,
            model_name,
            pattern,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(ProviderPage {
            items: rows.into_iter().map(|r| OllamaProviderForModel {
                provider_id: r.provider_id,
                name: r.name,
                url: r.url,
                status: r.status,
                is_enabled: r.is_enabled,
            }).collect(),
            total,
        })
    }

    async fn models_for_provider(&self, provider_id: Uuid) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT model_name FROM ollama_models WHERE provider_id = $1 ORDER BY model_name ASC LIMIT 10000",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}
