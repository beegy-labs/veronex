use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::ttl_cache::TtlCache;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::domain::entities::ApiKey;
use crate::domain::enums::KeyTier;

/// TTL for the per-hash API key cache (hot path: every inference request).
/// Mutations (revoke, deactivate, soft-delete, regenerate) call invalidate_all()
/// so stale entries are evicted immediately on any admin operation.
const API_KEY_CACHE_TTL: Duration = Duration::from_secs(60);

/// TTL-cache wrapper around any `ApiKeyRepository`.
///
/// `get_by_hash()` is the hot path — called by `infer_auth` on every
/// API-key-authenticated inference request.  The 60-second TTL collapses
/// repeated DB lookups for the same key into a single hit per TTL period.
///
/// All write operations (revoke, set_active, soft_delete, regenerate, …)
/// call `invalidate_all()` so policy changes take effect within at most
/// one in-flight TTL window (60 s) on any given instance.
pub struct CachingApiKeyRepo {
    inner: Arc<dyn ApiKeyRepository>,
    cache: TtlCache<String, Option<ApiKey>>,
}

impl CachingApiKeyRepo {
    pub fn new(inner: Arc<dyn ApiKeyRepository>) -> Self {
        Self {
            inner,
            cache: TtlCache::new(API_KEY_CACHE_TTL),
        }
    }
}

#[async_trait]
impl ApiKeyRepository for CachingApiKeyRepo {
    // ── Hot path ──────────────────────────────────────────────────────────────

    async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        self.cache
            .get_or_insert(
                key_hash.to_string(),
                self.inner.get_by_hash(key_hash),
            )
            .await
    }

    // ── Pass-through reads (not on hot path) ──────────────────────────────────

    async fn get_by_id(&self, key_id: &Uuid) -> Result<Option<ApiKey>> {
        self.inner.get_by_id(key_id).await
    }

    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<ApiKey>> {
        self.inner.list_by_tenant(tenant_id).await
    }

    async fn list_all(&self) -> Result<Vec<ApiKey>> {
        self.inner.list_all().await
    }

    async fn list_page(&self, search: &str, limit: i64, offset: i64) -> Result<(Vec<ApiKey>, i64)> {
        self.inner.list_page(search, limit, offset).await
    }

    async fn list_by_tenant_page(&self, tenant_id: &str, search: &str, limit: i64, offset: i64) -> Result<(Vec<ApiKey>, i64)> {
        self.inner.list_by_tenant_page(tenant_id, search, limit, offset).await
    }

    // ── Writes — invalidate cache so changes take effect immediately ──────────

    async fn create(&self, key: &ApiKey) -> Result<()> {
        self.inner.create(key).await
    }

    async fn revoke(&self, key_id: &Uuid) -> Result<()> {
        let result = self.inner.revoke(key_id).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn set_active(&self, key_id: &Uuid, active: bool) -> Result<()> {
        let result = self.inner.set_active(key_id, active).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn set_tier(&self, key_id: &Uuid, tier: &KeyTier) -> Result<()> {
        let result = self.inner.set_tier(key_id, tier).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn update_fields(&self, key_id: &Uuid, is_active: Option<bool>, tier: Option<&KeyTier>) -> Result<()> {
        let result = self.inner.update_fields(key_id, is_active, tier).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn soft_delete(&self, key_id: &Uuid) -> Result<()> {
        let result = self.inner.soft_delete(key_id).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn soft_delete_by_tenant(&self, tenant_id: &str) -> Result<u64> {
        let result = self.inner.soft_delete_by_tenant(tenant_id).await;
        self.cache.invalidate_all().await;
        result
    }

    async fn regenerate(&self, key_id: &Uuid, new_hash: &str, new_prefix: &str) -> Result<()> {
        let result = self.inner.regenerate(key_id, new_hash, new_prefix).await;
        self.cache.invalidate_all().await;
        result
    }
}
