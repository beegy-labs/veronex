use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct CapacitySettings {
    pub analyzer_model:      String,
    pub batch_enabled:       bool,
    pub batch_interval_secs: i32,
    pub last_run_at:         Option<DateTime<Utc>>,
    pub last_run_status:     Option<String>,
}

impl Default for CapacitySettings {
    fn default() -> Self {
        Self {
            analyzer_model:      "qwen2.5:3b".to_string(),
            batch_enabled:       true,
            batch_interval_secs: 300,
            last_run_at:         None,
            last_run_status:     None,
        }
    }
}

#[async_trait]
pub trait CapacitySettingsRepository: Send + Sync {
    async fn get(&self) -> Result<CapacitySettings>;
    async fn update_settings(
        &self,
        model: Option<&str>,
        batch_enabled: Option<bool>,
        interval_secs: Option<i32>,
    ) -> Result<CapacitySettings>;
    /// Record the result of the most recent analysis run (last_run_at = now()).
    async fn record_run(&self, status: &str) -> Result<()>;
}
