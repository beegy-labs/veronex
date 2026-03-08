use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::ttl_cache::TtlCache;
use crate::application::ports::outbound::ollama_model_repository::{
    OllamaModelRepository, OllamaModelWithCount, OllamaProviderForModel,
};

use crate::domain::constants::OLLAMA_MODEL_CACHE_TTL as TTL;

/// Thin TTL-cache wrapper around any `OllamaModelRepository`.
///
/// `providers_for_model(model)` is the hot path — called per Ollama dispatch
/// in the provider router.  A 10-second per-model TTL collapses repeated
/// queries for the same model into a single DB hit per TTL period.
///
/// `sync_provider_models` (the only write method) invalidates all entries
/// because a provider sync can change which models map to which providers.
pub struct CachingOllamaModelRepo {
    inner: Arc<dyn OllamaModelRepository>,
    cache: TtlCache<String, Vec<Uuid>>,
}

impl CachingOllamaModelRepo {
    pub fn new(inner: Arc<dyn OllamaModelRepository>) -> Self {
        Self {
            inner,
            cache: TtlCache::new(TTL),
        }
    }
}

#[async_trait]
impl OllamaModelRepository for CachingOllamaModelRepo {
    async fn sync_provider_models(
        &self,
        provider_id: Uuid,
        model_names: &[String],
    ) -> Result<()> {
        let result = self.inner.sync_provider_models(provider_id, model_names).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn list_all(&self) -> Result<Vec<String>> {
        self.inner.list_all().await
    }

    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>> {
        self.inner.list_with_counts().await
    }

    async fn providers_for_model(&self, model_name: &str) -> Result<Vec<Uuid>> {
        self.cache
            .get_or_insert(
                model_name.to_string(),
                self.inner.providers_for_model(model_name),
            )
            .await
    }

    async fn providers_info_for_model(
        &self,
        model_name: &str,
    ) -> Result<Vec<OllamaProviderForModel>> {
        self.inner.providers_info_for_model(model_name).await
    }

    async fn models_for_provider(&self, provider_id: Uuid) -> Result<Vec<String>> {
        self.inner.models_for_provider(provider_id).await
    }
}
