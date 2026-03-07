use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::application::ports::outbound::provider_dispatch_port::ProviderDispatchPort;
use crate::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::inference_provider::InferenceProviderPort;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::ProviderType;
use crate::infrastructure::outbound::provider_router::{
    get_ollama_available_vram_mb, increment_gemini_counters, make_adapter, pick_best_provider,
};

/// Concrete implementation of [`ProviderDispatchPort`].
///
/// Wraps the dynamic provider routing functions so the application use-case layer
/// can select providers and build adapters without importing infrastructure modules.
pub struct ConcreteProviderDispatch {
    registry: Arc<dyn LlmProviderRegistry>,
    gemini_policy_repo: Option<Arc<dyn GeminiPolicyRepository>>,
    model_selection_repo: Option<Arc<dyn ProviderModelSelectionRepository>>,
    ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
    valkey_pool: Option<fred::clients::Pool>,
}

impl ConcreteProviderDispatch {
    pub fn new(
        registry: Arc<dyn LlmProviderRegistry>,
        gemini_policy_repo: Option<Arc<dyn GeminiPolicyRepository>>,
        model_selection_repo: Option<Arc<dyn ProviderModelSelectionRepository>>,
        ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
        valkey_pool: Option<fred::clients::Pool>,
    ) -> Self {
        Self { registry, gemini_policy_repo, model_selection_repo, ollama_model_repo, valkey_pool }
    }
}

#[async_trait]
impl ProviderDispatchPort for ConcreteProviderDispatch {
    async fn available_vram_mb(&self, provider: &LlmProvider) -> i64 {
        get_ollama_available_vram_mb(provider, self.valkey_pool.as_ref()).await
    }

    fn build_adapter(&self, provider: &LlmProvider) -> Arc<dyn InferenceProviderPort> {
        make_adapter(provider)
    }

    async fn pick_and_build(
        &self,
        provider_type: &ProviderType,
        model_name: &str,
        tier_filter: Option<&str>,
    ) -> Result<(Arc<dyn InferenceProviderPort>, Uuid, bool)> {
        let cfg = pick_best_provider(
            &*self.registry,
            self.gemini_policy_repo.as_deref(),
            self.model_selection_repo.as_deref(),
            self.ollama_model_repo.as_deref(),
            provider_type,
            model_name,
            self.valkey_pool.as_ref(),
            tier_filter,
        )
        .await?;
        let provider_id = cfg.id;
        let is_free_tier = cfg.is_free_tier;
        Ok((make_adapter(&cfg), provider_id, is_free_tier))
    }

    async fn increment_gemini_counters(&self, provider_id: Uuid, model: &str) -> Result<()> {
        if let Some(ref pool) = self.valkey_pool {
            increment_gemini_counters(pool, provider_id, model).await
        } else {
            Ok(())
        }
    }
}
