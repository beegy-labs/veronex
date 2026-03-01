use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::LlmBackend;
use crate::domain::enums::LlmBackendStatus;

/// Thin TTL-cache wrapper around any `LlmBackendRegistry` implementation.
///
/// `list_all()` is the hot path — called on every job dispatch from the queue.
/// Under load, hundreds of calls/second would otherwise hammer Postgres.
/// A 5-second TTL makes the query once per TTL period instead of once per job.
///
/// All mutating methods (`register`, `update_status`, `deactivate`, `update`)
/// invalidate the cache immediately so stale data is never served after a write.
pub struct CachingBackendRegistry {
    inner: Arc<dyn LlmBackendRegistry>,
    cache: RwLock<Option<(Vec<LlmBackend>, Instant)>>,
    ttl:   Duration,
}

impl CachingBackendRegistry {
    pub fn new(inner: Arc<dyn LlmBackendRegistry>, ttl: Duration) -> Self {
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
impl LlmBackendRegistry for CachingBackendRegistry {
    async fn register(&self, backend: &LlmBackend) -> Result<()> {
        let result = self.inner.register(backend).await;
        self.invalidate().await;
        result
    }

    async fn list_active(&self) -> Result<Vec<LlmBackend>> {
        // Infrequent management path — forward directly, no caching needed.
        self.inner.list_active().await
    }

    async fn list_all(&self) -> Result<Vec<LlmBackend>> {
        // Fast path: shared read lock, return cached value if still fresh.
        {
            let guard = self.cache.read().await;
            if let Some((ref backends, ts)) = *guard {
                if ts.elapsed() < self.ttl {
                    return Ok(backends.clone());
                }
            }
        }

        // Cache miss or stale: acquire write lock, re-check, then refresh.
        let mut guard = self.cache.write().await;
        // Re-check under write lock to avoid thundering-herd.
        if let Some((ref backends, ts)) = *guard {
            if ts.elapsed() < self.ttl {
                return Ok(backends.clone());
            }
        }
        let fresh = self.inner.list_all().await?;
        *guard = Some((fresh.clone(), Instant::now()));
        Ok(fresh)
    }

    async fn get(&self, id: Uuid) -> Result<Option<LlmBackend>> {
        // Infrequent management path — forward directly.
        self.inner.get(id).await
    }

    async fn update_status(&self, id: Uuid, status: LlmBackendStatus) -> Result<()> {
        let result = self.inner.update_status(id, status).await;
        self.invalidate().await;
        result
    }

    async fn deactivate(&self, id: Uuid) -> Result<()> {
        let result = self.inner.deactivate(id).await;
        self.invalidate().await;
        result
    }

    async fn update(&self, backend: &LlmBackend) -> Result<()> {
        let result = self.inner.update(backend).await;
        self.invalidate().await;
        result
    }
}
