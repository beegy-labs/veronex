use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct CapacitySettings {
    pub analyzer_model:     String,
    pub sync_enabled:       bool,
    pub sync_interval_secs: i32,
    /// AIMD probe: extra (+) or fewer (-) concurrent requests for learning.
    pub probe_permits:      i32,
    /// AIMD probe: every N arrivals at the limit, apply probe_permits.
    pub probe_rate:         i32,
    pub last_run_at:        Option<DateTime<Utc>>,
    pub last_run_status:    Option<String>,
}

impl Default for CapacitySettings {
    fn default() -> Self {
        Self {
            analyzer_model:     "qwen2.5:3b".to_string(),
            sync_enabled:       true,
            sync_interval_secs: 300,
            probe_permits:      1,
            probe_rate:         3,
            last_run_at:        None,
            last_run_status:    None,
        }
    }
}

#[async_trait]
pub trait CapacitySettingsRepository: Send + Sync {
    async fn get(&self) -> Result<CapacitySettings>;
    async fn update_settings(
        &self,
        model: Option<&str>,
        sync_enabled: Option<bool>,
        interval_secs: Option<i32>,
        probe_permits: Option<i32>,
        probe_rate: Option<i32>,
    ) -> Result<CapacitySettings>;
    /// Record the result of the most recent sync run (last_run_at = now()).
    async fn record_run(&self, status: &str) -> Result<()>;
}
