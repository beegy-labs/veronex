use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::application::ports::outbound::inference_backend::InferenceBackendPort;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::ProviderType;

/// Port for provider selection, adapter construction, and rate-limit counter management.
///
/// Abstracts over the concrete routing logic (`pick_best_provider`, `make_adapter`,
/// `get_ollama_available_vram_mb`, `increment_gemini_counters`) so the application
/// use-case layer does not depend on infrastructure adapters directly.
#[async_trait]
pub trait ProviderDispatchPort: Send + Sync {
    /// Available VRAM in MiB for the given provider.
    ///
    /// Returns `i64::MAX` for unconstrained/unknown providers, `i64::MIN` when
    /// overheating, and `0` on network/parse error.
    async fn available_vram_mb(&self, provider: &LlmProvider) -> i64;

    /// Build a concrete adapter from a provider DB record.
    fn build_adapter(&self, provider: &LlmProvider) -> Arc<dyn InferenceBackendPort>;

    /// Pick the best provider for the given type and model, then build an adapter.
    ///
    /// Returns `(adapter, provider_id, is_free_tier)`.
    async fn pick_and_build(
        &self,
        provider_type: &ProviderType,
        model_name: &str,
        tier_filter: Option<&str>,
    ) -> Result<(Arc<dyn InferenceBackendPort>, Uuid, bool)>;

    /// Increment Gemini RPM/RPD rate-limit counters after a successful inference.
    ///
    /// No-op when Valkey is not configured (fail-open).
    async fn increment_gemini_counters(&self, provider_id: Uuid, model: &str) -> Result<()>;
}
