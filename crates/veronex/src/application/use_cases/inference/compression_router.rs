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
    /// Provider ID (for logging).
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
