use std::sync::Arc;
use uuid::Uuid;

use crate::application::ports::outbound::lab_settings_repository::{LabSettings, LabSettingsRepository};
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::enums::ProviderType;

// ── Public handle ────────────────────────────────────────────────────────────

/// Resources needed for per-turn compression, stored in `JobEntry` and read
/// by `finalize_job()` after S3 write.
pub struct CompressionHandle {
    pub registry: Arc<dyn LlmProviderRegistry>,
    pub lab_settings: Arc<dyn LabSettingsRepository>,
}

// ── Route decision ───────────────────────────────────────────────────────────

/// Routing decision for where (and when) to run per-turn compression.
#[derive(Debug)]
pub enum CompressionRoute {
    /// Single provider, or compression disabled / not applicable.
    /// Compression is deferred to Turn N+1 context assembly (Phase 4).
    SyncInline,
    /// Two+ providers, no dedicated model set. Compress async to the given provider.
    AsyncIdle { provider_id: Uuid, provider_url: String },
    /// Dedicated compression model configured. Compress async to the designated provider.
    AsyncDedicated { provider_id: Uuid, provider_url: String },
    /// All providers saturated or unavailable. Skip; retry deferred to next turn.
    Skip,
}

/// Parameters passed to `compress_turn()`.
pub struct CompressParams {
    /// Compression model name (e.g. `"qwen2.5:3b"`).
    pub model: String,
    /// Base URL of the target Ollama provider.
    pub provider_url: String,
    /// Provider ID (for logging/tests).
    #[allow(dead_code)]
    pub provider_id: Uuid,
    /// Per-call timeout in seconds.
    pub timeout_secs: u64,
}

impl CompressionRoute {
    /// Extract `CompressParams` for async routes; returns `None` for `SyncInline`/`Skip`.
    pub fn into_params(self, model: String, timeout_secs: u64) -> Option<CompressParams> {
        match self {
            CompressionRoute::AsyncDedicated { provider_id, provider_url }
            | CompressionRoute::AsyncIdle { provider_id, provider_url } => Some(CompressParams {
                model,
                provider_url,
                provider_id,
                timeout_secs,
            }),
            _ => None,
        }
    }
}

/// Decide where to run compression for the just-completed turn.
///
/// Decision priority (matches SDD §CompressionRouter Policy):
/// 1. `lab.compression_model` set → `AsyncDedicated` (first active Ollama provider)
/// 2. Single Ollama provider → `SyncInline` (deferred to Phase 4 context assembly)
/// 3. Multiple providers → `AsyncIdle` (first active Ollama provider)
/// 4. No active Ollama providers → `Skip`
pub async fn decide(
    registry: &dyn LlmProviderRegistry,
    lab: &LabSettings,
) -> CompressionRoute {
    let providers = match registry.list_active().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("compression_router: registry error: {e}");
            return CompressionRoute::Skip;
        }
    };

    let ollama: Vec<_> = providers
        .into_iter()
        .filter(|p| p.provider_type == ProviderType::Ollama)
        .collect();

    if ollama.is_empty() {
        return CompressionRoute::Skip;
    }

    // Priority 1: dedicated compression model → route to first active Ollama provider
    if lab.compression_model.is_some() {
        if let Some(p) = ollama.first() {
            return CompressionRoute::AsyncDedicated {
                provider_id: p.id,
                provider_url: p.url.clone(),
            };
        }
    }

    // Priority 2: single provider → defer to Phase 4 inline
    if ollama.len() == 1 {
        return CompressionRoute::SyncInline;
    }

    // Priority 3: multiple providers → pick first (per-provider active_requests tracking
    // is a future enhancement; skip-on-busy logic added in Phase 4)
    if let Some(p) = ollama.first() {
        return CompressionRoute::AsyncIdle {
            provider_id: p.id,
            provider_url: p.url.clone(),
        };
    }

    CompressionRoute::Skip
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::application::ports::outbound::lab_settings_repository::LabSettings;
    use crate::domain::entities::LlmProvider;
    use crate::domain::enums::{LlmProviderStatus, ProviderType};

    fn ollama_provider(id: Uuid, url: &str) -> LlmProvider {
        LlmProvider {
            id,
            name: "test".to_string(),
            provider_type: ProviderType::Ollama,
            url: url.to_string(),
            api_key_encrypted: None,
            is_active: true,
            total_vram_mb: 0,
            gpu_index: None,
            server_id: None,
            is_free_tier: false,
            num_parallel: 4,
            status: LlmProviderStatus::Online,
            registered_at: chrono::Utc::now(),
        }
    }

    struct MockRegistry(Vec<LlmProvider>);

    #[async_trait]
    impl crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry for MockRegistry {
        async fn register(&self, _: &LlmProvider) -> anyhow::Result<()> { Ok(()) }
        async fn list_active(&self) -> anyhow::Result<Vec<LlmProvider>> { Ok(self.0.clone()) }
        async fn list_all(&self) -> anyhow::Result<Vec<LlmProvider>> { Ok(self.0.clone()) }
        async fn list_page(&self, _: &str, _: Option<&str>, _: i64, _: i64) -> anyhow::Result<(Vec<LlmProvider>, i64)> { Ok((self.0.clone(), self.0.len() as i64)) }
        async fn get(&self, _: Uuid) -> anyhow::Result<Option<LlmProvider>> { Ok(None) }
        async fn update_status(&self, _: Uuid, _: LlmProviderStatus) -> anyhow::Result<()> { Ok(()) }
        async fn deactivate(&self, _: Uuid) -> anyhow::Result<()> { Ok(()) }
        async fn update(&self, _: &LlmProvider) -> anyhow::Result<()> { Ok(()) }
    }

    fn lab_no_model() -> LabSettings { LabSettings::default() }
    fn lab_with_model() -> LabSettings {
        LabSettings { compression_model: Some("qwen2.5:3b".to_string()), ..Default::default() }
    }

    #[tokio::test]
    async fn no_providers_returns_skip() {
        let reg = MockRegistry(vec![]);
        let route = decide(&reg, &lab_no_model()).await;
        assert!(matches!(route, CompressionRoute::Skip));
    }

    #[tokio::test]
    async fn single_provider_no_model_returns_sync_inline() {
        let reg = MockRegistry(vec![ollama_provider(Uuid::now_v7(), "http://localhost:11434")]);
        let route = decide(&reg, &lab_no_model()).await;
        assert!(matches!(route, CompressionRoute::SyncInline));
    }

    #[tokio::test]
    async fn single_provider_with_dedicated_model_returns_async_dedicated() {
        let id = Uuid::now_v7();
        let reg = MockRegistry(vec![ollama_provider(id, "http://localhost:11434")]);
        let route = decide(&reg, &lab_with_model()).await;
        assert!(matches!(route, CompressionRoute::AsyncDedicated { .. }));
        if let CompressionRoute::AsyncDedicated { provider_id, .. } = route {
            assert_eq!(provider_id, id);
        }
    }

    #[tokio::test]
    async fn multiple_providers_no_model_returns_async_idle() {
        let id = Uuid::now_v7();
        let reg = MockRegistry(vec![
            ollama_provider(id, "http://host1:11434"),
            ollama_provider(Uuid::now_v7(), "http://host2:11434"),
        ]);
        let route = decide(&reg, &lab_no_model()).await;
        assert!(matches!(route, CompressionRoute::AsyncIdle { .. }));
        if let CompressionRoute::AsyncIdle { provider_id, .. } = route {
            assert_eq!(provider_id, id);
        }
    }

    #[tokio::test]
    async fn multiple_providers_with_model_returns_async_dedicated() {
        let id = Uuid::now_v7();
        let reg = MockRegistry(vec![
            ollama_provider(id, "http://host1:11434"),
            ollama_provider(Uuid::now_v7(), "http://host2:11434"),
        ]);
        let route = decide(&reg, &lab_with_model()).await;
        assert!(matches!(route, CompressionRoute::AsyncDedicated { .. }));
    }

    #[test]
    fn into_params_sync_inline_returns_none() {
        assert!(CompressionRoute::SyncInline.into_params("m".to_string(), 30).is_none());
    }

    #[test]
    fn into_params_skip_returns_none() {
        assert!(CompressionRoute::Skip.into_params("m".to_string(), 30).is_none());
    }

    #[test]
    fn into_params_async_idle_returns_some() {
        let id = Uuid::now_v7();
        let route = CompressionRoute::AsyncIdle { provider_id: id, provider_url: "http://x".to_string() };
        let p = route.into_params("qwen2.5:3b".to_string(), 30).unwrap();
        assert_eq!(p.model, "qwen2.5:3b");
        assert_eq!(p.provider_id, id);
        assert_eq!(p.timeout_secs, 30);
    }

    #[test]
    fn into_params_async_dedicated_returns_some() {
        let id = Uuid::now_v7();
        let route = CompressionRoute::AsyncDedicated { provider_id: id, provider_url: "http://x".to_string() };
        let p = route.into_params("qwen2.5:3b".to_string(), 60).unwrap();
        assert_eq!(p.provider_id, id);
        assert_eq!(p.timeout_secs, 60);
    }
}
