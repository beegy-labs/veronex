//! Outbound ports for Gemini provider integration.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::GeminiRateLimitPolicy;

// ── Sync config ────────────────────────────────────────────────────────────────

#[async_trait]
pub trait GeminiSyncConfigRepository: Send + Sync {
    /// Returns `None` if no key has been set yet.
    async fn get_api_key(&self) -> Result<Option<String>>;

    /// Upsert the admin API key.
    async fn set_api_key(&self, api_key: &str) -> Result<()>;
}

// ── Models ─────────────────────────────────────────────────────────────────────

pub struct GeminiModel {
    pub model_name: String,
    pub synced_at: DateTime<Utc>,
}

#[async_trait]
pub trait GeminiModelRepository: Send + Sync {
    /// Replace the global model pool: DELETE all + INSERT the new list.
    async fn sync_models(&self, model_names: &[String]) -> Result<()>;

    /// List all global Gemini models ordered by name.
    async fn list(&self) -> Result<Vec<GeminiModel>>;
}

// ── Rate-limit policies ────────────────────────────────────────────────────────

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
