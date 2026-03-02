use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::LlmProviderStatus;

/// Thin TTL-cache wrapper around any `LlmProviderRegistry` implementation.
///
/// `list_all()` is the hot path — called on every job dispatch from the queue.
/// Under load, hundreds of calls/second would otherwise hammer Postgres.
/// A 5-second TTL makes the query once per TTL period instead of once per job.
///
/// All mutating methods (`register`, `update_status`, `deactivate`, `update`)
/// invalidate the cache immediately so stale data is never served after a write.
pub struct CachingProviderRegistry {
    inner: Arc<dyn LlmProviderRegistry>,
    cache: RwLock<Option<(Vec<LlmProvider>, Instant)>>,
    ttl:   Duration,
}

impl CachingProviderRegistry {
    pub fn new(inner: Arc<dyn LlmProviderRegistry>, ttl: Duration) -> Self {
        Self {
            inner,
            cache: RwLock::new(None),
            ttl,
        }
    }

    /// Invalidate the cached list so the next `list_all` hits the DB.
    async fn invalidate(&self) {
        *self.cache.write().await = None;
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
        // Fast path: shared read lock, return cached value if still fresh.
        {
            let guard = self.cache.read().await;
            if let Some((ref providers, ts)) = *guard {
                if ts.elapsed() < self.ttl {
                    return Ok(providers.clone());
                }
            }
        }

        // Cache miss or stale: acquire write lock, re-check, then refresh.
        let mut guard = self.cache.write().await;
        // Re-check under write lock to avoid thundering-herd.
        if let Some((ref providers, ts)) = *guard {
            if ts.elapsed() < self.ttl {
                return Ok(providers.clone());
            }
        }
        let fresh = self.inner.list_all().await?;
        *guard = Some((fresh.clone(), Instant::now()));
        Ok(fresh)
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

    async fn deactivate(&self, id: Uuid) -> Result<()> {
        let result = self.inner.deactivate(id).await;
        self.invalidate().await;
        result
    }

    async fn update(&self, provider: &LlmProvider) -> Result<()> {
        let result = self.inner.update(provider).await;
        self.invalidate().await;
        result
    }
}
