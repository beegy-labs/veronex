use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;

use crate::application::ports::outbound::mcp_settings_repository::{
    McpSettings, McpSettingsRepository, McpSettingsUpdate,
};

pub struct PostgresMcpSettingsRepository {
    pool: PgPool,
}

impl PostgresMcpSettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl McpSettingsRepository for PostgresMcpSettingsRepository {
    async fn get(&self) -> Result<McpSettings> {
        #[derive(sqlx::FromRow)]
        struct Row {
            routing_cache_ttl_secs: i32,
            tool_schema_refresh_secs: i32,
            embedding_model: String,
            max_tools_per_request: i32,
            max_routing_cache_entries: i32,
            updated_at: chrono::DateTime<chrono::Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            "SELECT routing_cache_ttl_secs, tool_schema_refresh_secs, embedding_model,
                    max_tools_per_request, max_routing_cache_entries, updated_at
             FROM mcp_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed to get mcp_settings")?;

        Ok(row
            .map(|r| McpSettings {
                routing_cache_ttl_secs: r.routing_cache_ttl_secs,
                tool_schema_refresh_secs: r.tool_schema_refresh_secs,
                embedding_model: r.embedding_model,
                max_tools_per_request: r.max_tools_per_request,
                max_routing_cache_entries: r.max_routing_cache_entries,
                updated_at: r.updated_at,
            })
            .unwrap_or_default())
    }

    async fn update(&self, patch: McpSettingsUpdate) -> Result<McpSettings> {
        sqlx::query(
            "UPDATE mcp_settings SET
                routing_cache_ttl_secs    = COALESCE($1, routing_cache_ttl_secs),
                tool_schema_refresh_secs  = COALESCE($2, tool_schema_refresh_secs),
                embedding_model           = COALESCE($3, embedding_model),
                max_tools_per_request     = COALESCE($4, max_tools_per_request),
                max_routing_cache_entries = COALESCE($5, max_routing_cache_entries),
                updated_at                = now()
             WHERE id = 1",
        )
        .bind(patch.routing_cache_ttl_secs)
        .bind(patch.tool_schema_refresh_secs)
        .bind(patch.embedding_model)
        .bind(patch.max_tools_per_request)
        .bind(patch.max_routing_cache_entries)
        .execute(&self.pool)
        .await
        .context("failed to update mcp_settings")?;

        self.get().await
    }
}
