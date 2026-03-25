use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Snapshot of all lab (experimental) feature flags.
#[derive(Debug, Clone)]
pub struct LabSettings {
    /// Gemini function-calling (tool use) support.
    pub gemini_function_calling: bool,
    /// Max images per inference request. 0 = image input disabled.
    pub max_images_per_request: i32,
    /// Max base64 bytes per image (default 2MB).
    pub max_image_b64_bytes: i32,
    /// Ollama model used as the MCP orchestrator. None = use the model from the request.
    pub mcp_orchestrator_model: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for LabSettings {
    fn default() -> Self {
        Self {
            gemini_function_calling: false,
            max_images_per_request: 4,
            max_image_b64_bytes: 2 * 1024 * 1024,
            mcp_orchestrator_model: None,
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
        max_images_per_request: Option<i32>,
        max_image_b64_bytes: Option<i32>,
        mcp_orchestrator_model: Option<Option<String>>,
    ) -> Result<LabSettings>;
}
