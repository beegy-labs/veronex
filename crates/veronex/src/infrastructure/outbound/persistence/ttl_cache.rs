use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// Generic double-checked TTL cache backed by `RwLock<HashMap<K, (V, Instant)>>`.
///
/// Provides O(1) read-lock fast path and write-lock slow path with re-check
/// to avoid thundering-herd on cache miss.  Used by all caching persistence
/// wrappers (`CachingOllamaModelRepo`, `CachingModelSelection`,
/// `CachingProviderRegistry`).
pub struct TtlCache<K: Eq + Hash, V: Clone> {
    inner: RwLock<HashMap<K, (V, Instant)>>,
    ttl: Duration,
}

impl<K: Eq + Hash, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Return cached value if present and fresh.
    pub async fn get(&self, key: &K) -> Option<V> {
        let cache = self.inner.read().await;
        cache
            .get(key)
            .and_then(|(v, ts)| (ts.elapsed() < self.ttl).then(|| v.clone()))
    }

    /// Double-checked get-or-insert: fast read path, slow write path with
    /// re-check, then `fetch` on miss.
    pub async fn get_or_insert<F, E>(&self, key: K, fetch: F) -> Result<V, E>
    where
        F: std::future::Future<Output = Result<V, E>>,
        K: Clone,
    {
        // Fast path: read lock.
        {
            let cache = self.inner.read().await;
            if let Some((v, ts)) = cache.get(&key) {
                if ts.elapsed() < self.ttl {
                    return Ok(v.clone());
                }
            }
        }

        // Slow path: write lock with re-check.
        let mut cache = self.inner.write().await;
        if let Some((v, ts)) = cache.get(&key) {
            if ts.elapsed() < self.ttl {
                return Ok(v.clone());
            }
        }
        let value = fetch.await?;
        cache.insert(key, (value.clone(), Instant::now()));
        Ok(value)
    }

    /// Remove a single key, returning whether it was present.
    pub async fn invalidate(&self, key: &K) {
        self.inner.write().await.remove(key);
    }

    /// Remove all entries.
    pub async fn invalidate_all(&self) {
        self.inner.write().await.clear();
    }
}
