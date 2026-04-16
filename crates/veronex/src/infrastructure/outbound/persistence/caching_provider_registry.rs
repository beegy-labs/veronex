use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::ttl_cache::TtlCache;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::LlmProviderStatus;

/// Thin TTL-cache wrapper around any `LlmProviderRegistry` implementation.
///
/// `list_all()` is the hot path — called on every job dispatch from the queue.
/// Under load, hundreds of calls/second would otherwise hammer Postgres.
/// A 5-second TTL makes the query once per TTL period instead of once per job.
///
/// All mutating methods (`register`, `update_status`, `delete`, `update`)
/// invalidate the cache immediately so stale data is never served after a write.
pub struct CachingProviderRegistry {
    inner: Arc<dyn LlmProviderRegistry>,
    /// Single-entry cache keyed on `()`.
    cache: TtlCache<(), Vec<LlmProvider>>,
}

impl CachingProviderRegistry {
    pub fn new(inner: Arc<dyn LlmProviderRegistry>, ttl: Duration) -> Self {
        Self {
            inner,
            cache: TtlCache::new(ttl),
        }
    }

    async fn invalidate(&self) {
        self.cache.invalidate_all().await;
    }
}

#[async_trait]
impl LlmProviderRegistry for CachingProviderRegistry {
    async fn register(&self, provider: &LlmProvider) -> Result<()> {
        let result = self.inner.register(provider).await;
        self.invalidate().await;
        result
    }

    async fn list_active(&self) -> Result<Vec<LlmProvider>> {
        // Infrequent management path — forward directly, no caching needed.
        self.inner.list_active().await
    }

    async fn list_all(&self) -> Result<Vec<LlmProvider>> {
        self.cache
            .get_or_insert((), self.inner.list_all())
            .await
    }

    async fn list_page(&self, search: &str, provider_type: Option<&str>, limit: i64, offset: i64) -> Result<(Vec<LlmProvider>, i64)> {
        // Management path — forward directly, no caching needed.
        self.inner.list_page(search, provider_type, limit, offset).await
    }

    async fn get(&self, id: Uuid) -> Result<Option<LlmProvider>> {
        // Infrequent management path — forward directly.
        self.inner.get(id).await
    }

    async fn update_status(&self, id: Uuid, status: LlmProviderStatus) -> Result<()> {
        let result = self.inner.update_status(id, status).await;
        self.invalidate().await;
        result
    }

    async fn delete(&self, id: Uuid) -> Result<()> {
        let result = self.inner.delete(id).await;
        self.invalidate().await;
        result
    }

    async fn update(&self, provider: &LlmProvider) -> Result<()> {
        let result = self.inner.update(provider).await;
        self.invalidate().await;
        result
    }
}
