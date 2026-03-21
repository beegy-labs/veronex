use async_trait::async_trait;

/// Global model enable/disable settings.
/// When a model is globally disabled, it is blocked on ALL providers
/// regardless of per-provider selected_models state.
#[derive(Debug, Clone)]
pub struct GlobalModelSetting {
    pub model_name: String,
    pub is_enabled: bool,
}

#[async_trait]
pub trait GlobalModelSettingsRepository: Send + Sync {
    /// List all global model settings (only models with explicit settings).
    async fn list(&self) -> anyhow::Result<Vec<GlobalModelSetting>>;

    /// Check if a model is globally disabled.
    /// Returns `true` if the model has no setting (default enabled) or is explicitly enabled.
    async fn is_enabled(&self, model_name: &str) -> anyhow::Result<bool>;

    /// Set global enable/disable for a model (upsert).
    async fn set_enabled(&self, model_name: &str, enabled: bool) -> anyhow::Result<()>;

    /// List all globally disabled model names.
    async fn list_disabled(&self) -> anyhow::Result<Vec<String>>;
}
