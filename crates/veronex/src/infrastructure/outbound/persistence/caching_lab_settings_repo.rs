use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use super::ttl_cache::TtlCache;
use crate::application::ports::outbound::lab_settings_repository::{
    LabSettings, LabSettingsRepository, LabSettingsUpdate,
};

/// TTL for the lab settings cache.
/// lab_settings is a single admin-only row updated rarely.
/// 30 s balances freshness vs DB load.
const LAB_SETTINGS_CACHE_TTL: Duration = Duration::from_secs(30);

/// TTL-cache wrapper around any `LabSettingsRepository`.
///
/// `get()` is called on every image-bearing request (line 293) and every
/// MCP request (line 604) in `openai_handlers.rs`.  Without caching this
/// hits the DB on every such request; the 30-second TTL collapses repeated
/// reads of the same single-row config table into one DB hit per window.
///
/// `update()` calls `invalidate_all()` so the new value is visible on the
/// next request rather than waiting for TTL expiry.
pub struct CachingLabSettingsRepo {
    inner: Arc<dyn LabSettingsRepository>,
    cache: TtlCache<(), LabSettings>,
}

impl CachingLabSettingsRepo {
    pub fn new(inner: Arc<dyn LabSettingsRepository>) -> Self {
        Self {
            inner,
            cache: TtlCache::new(LAB_SETTINGS_CACHE_TTL),
        }
    }
}

#[async_trait]
impl LabSettingsRepository for CachingLabSettingsRepo {
    async fn get(&self) -> Result<LabSettings> {
        self.cache
            .get_or_insert((), self.inner.get())
            .await
    }

    async fn update(&self, patch: LabSettingsUpdate) -> Result<LabSettings> {
        let result = self.inner.update(patch).await;
        self.cache.invalidate_all().await;
        result
    }
}
