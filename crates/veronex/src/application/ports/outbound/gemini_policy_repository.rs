use anyhow::Result;
use async_trait::async_trait;

use crate::domain::entities::GeminiRateLimitPolicy;

#[async_trait]
pub trait GeminiPolicyRepository: Send + Sync {
    /// All policies (used for admin list endpoint).
    async fn list_all(&self) -> Result<Vec<GeminiRateLimitPolicy>>;

    /// Look up policy for a specific model.
    /// Falls back to the "*" global default if no model-specific row exists.
    async fn get_for_model(&self, model_name: &str) -> Result<Option<GeminiRateLimitPolicy>>;

    /// Insert or update a policy row (upsert on model_name).
    async fn upsert(&self, policy: &GeminiRateLimitPolicy) -> Result<()>;
}
