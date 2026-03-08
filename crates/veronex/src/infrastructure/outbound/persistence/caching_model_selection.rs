use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::ttl_cache::TtlCache;
use crate::application::ports::outbound::provider_model_selection::{
    ProviderModelSelectionRepository, ProviderSelectedModel,
};

use crate::domain::constants::MODEL_SELECTION_CACHE_TTL as TTL;

/// Thin TTL-cache wrapper around any `ProviderModelSelectionRepository`.
///
/// `list_enabled(provider_id)` is the hot path — called per provider
/// candidate in the provider router (~312, ~384).  A 30-second per-provider
/// TTL collapses repeated queries into a single DB hit per TTL period.
/// Model selection changes infrequently, so a longer TTL is appropriate.
///
/// Write methods (`upsert_models`, `set_enabled`) invalidate the cache entry
/// for the affected `provider_id` so stale data is never served after a write.
pub struct CachingModelSelection {
    inner: Arc<dyn ProviderModelSelectionRepository>,
    cache: TtlCache<Uuid, Vec<String>>,
}

impl CachingModelSelection {
    pub fn new(inner: Arc<dyn ProviderModelSelectionRepository>) -> Self {
        Self {
            inner,
            cache: TtlCache::new(TTL),
        }
    }
}

#[async_trait]
impl ProviderModelSelectionRepository for CachingModelSelection {
    async fn upsert_models(&self, provider_id: Uuid, models: &[String]) -> Result<()> {
        let result = self.inner.upsert_models(provider_id, models).await;
        self.cache.invalidate(&provider_id).await;
        result
    }

    async fn list(&self, provider_id: Uuid) -> Result<Vec<ProviderSelectedModel>> {
        // Management path — forward directly, no caching needed.
        self.inner.list(provider_id).await
    }

    async fn set_enabled(
        &self,
        provider_id: Uuid,
        model_name: &str,
        enabled: bool,
    ) -> Result<()> {
        let result = self.inner.set_enabled(provider_id, model_name, enabled).await;
        self.cache.invalidate(&provider_id).await;
        result
    }

    async fn list_enabled(&self, provider_id: Uuid) -> Result<Vec<String>> {
        self.cache
            .get_or_insert(provider_id, self.inner.list_enabled(provider_id))
            .await
    }
}
