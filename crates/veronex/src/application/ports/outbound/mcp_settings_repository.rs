use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct McpSettings {
    pub routing_cache_ttl_secs: i32,
    pub tool_schema_refresh_secs: i32,
    pub embedding_model: String,
    pub max_tools_per_request: i32,
    pub max_routing_cache_entries: i32,
    pub updated_at: DateTime<Utc>,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            routing_cache_ttl_secs: 300,
            tool_schema_refresh_secs: 3600,
            embedding_model: "nomic-embed-text".to_string(),
            max_tools_per_request: 20,
            max_routing_cache_entries: 1000,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Default)]
pub struct McpSettingsUpdate {
    pub routing_cache_ttl_secs: Option<i32>,
    pub tool_schema_refresh_secs: Option<i32>,
    pub embedding_model: Option<String>,
    pub max_tools_per_request: Option<i32>,
    pub max_routing_cache_entries: Option<i32>,
}

#[async_trait]
pub trait McpSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<McpSettings>;
    async fn update(&self, patch: McpSettingsUpdate) -> Result<McpSettings>;
}
