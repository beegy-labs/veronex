use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Snapshot of all lab (experimental) feature flags.
#[derive(Debug, Clone)]
pub struct LabSettings {
    /// Gemini function-calling (tool use) support.
    /// Disabled by default — still in active development.
    pub gemini_function_calling: bool,
    pub updated_at: DateTime<Utc>,
}

impl Default for LabSettings {
    fn default() -> Self {
        Self {
            gemini_function_calling: false,
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
pub trait LabSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<LabSettings>;
    async fn update(
        &self,
        gemini_function_calling: Option<bool>,
    ) -> Result<LabSettings>;
}
