//! Per-model context size resolution.
//!
//! SDD: `.specs/veronex/history/conversation-context-compression.md` §3 (Tier A).
//!
//! Replaces hardcoded `configured_ctx = 32_768u32` fallback in inline
//! compression budget calculations. Sources the value from
//! `model_vram_profiles.configured_ctx` populated by the capacity analyzer.

use uuid::Uuid;

use crate::application::ports::outbound::model_capacity_repository::ModelCapacityRepository;

/// Sanity floor — values below this are treated as unset / corrupt.
const CTX_SANITY_FLOOR: u32 = 4096;

/// Legacy fallback when no profile row exists.
const CTX_LEGACY_FALLBACK: u32 = 32_768;

/// Resolve the configured context window for `(provider_id, model)`.
///
/// Lookup order:
/// 1. `model_vram_profiles.configured_ctx` (capacity analyzer output)
/// 2. `CTX_LEGACY_FALLBACK` (32_768) when no profile exists or value is below
///    `CTX_SANITY_FLOOR` — preserves pre-S17 behavior on unknown models.
///
/// Always returns a non-zero value safe to feed into budget arithmetic.
pub async fn resolve_model_context_size(
    capacity_repo: &dyn ModelCapacityRepository,
    provider_id: Uuid,
    model: &str,
) -> u32 {
    capacity_repo
        .get(provider_id, model)
        .await
        .ok()
        .flatten()
        .map(|p| p.configured_ctx.max(0) as u32)
        .filter(|&c| c >= CTX_SANITY_FLOOR)
        .unwrap_or(CTX_LEGACY_FALLBACK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::outbound::model_capacity_repository::{
        ModelCapacityRepository, ModelVramProfileEntry, ThroughputStats,
    };
    use anyhow::Result;
    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    struct StubRepo {
        entry: Option<ModelVramProfileEntry>,
    }

    #[async_trait]
    impl ModelCapacityRepository for StubRepo {
        async fn upsert(&self, _: &ModelVramProfileEntry) -> Result<()> {
            Ok(())
        }
        async fn get(&self, _: Uuid, _: &str) -> Result<Option<ModelVramProfileEntry>> {
            Ok(self.entry.clone())
        }
        async fn list_all(&self) -> Result<Vec<ModelVramProfileEntry>> {
            Ok(self.entry.clone().into_iter().collect())
        }
        async fn list_by_provider(&self, _: Uuid) -> Result<Vec<ModelVramProfileEntry>> {
            Ok(vec![])
        }
        async fn list_by_providers(&self, _: &[Uuid]) -> Result<Vec<ModelVramProfileEntry>> {
            Ok(vec![])
        }
        async fn compute_throughput_stats(
            &self,
            _: Uuid,
            _: &str,
            _: u32,
        ) -> Result<Option<ThroughputStats>> {
            Ok(None)
        }
        async fn has_unprofiled_selected_models(&self) -> Result<bool> {
            Ok(false)
        }
        async fn min_configured_ctx_for_model(&self, _: &str) -> Result<Option<u32>> {
            Ok(self
                .entry
                .as_ref()
                .map(|e| e.configured_ctx.max(0) as u32))
        }
    }

    fn make_entry(configured_ctx: i32) -> ModelVramProfileEntry {
        ModelVramProfileEntry {
            provider_id: Uuid::nil(),
            model_name: "x".into(),
            weight_mb: 0,
            weight_estimated: false,
            kv_per_request_mb: 0,
            num_layers: 0,
            num_kv_heads: 0,
            head_dim: 0,
            configured_ctx,
            max_ctx: configured_ctx,
            failure_count: 0,
            llm_concern: None,
            llm_reason: None,
            max_concurrent: 1,
            baseline_tps: 0,
            baseline_p95_ms: 0,
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn returns_db_value_for_known_model() {
        let repo = StubRepo {
            entry: Some(make_entry(262_144)),
        };
        let n = resolve_model_context_size(&repo, Uuid::nil(), "qwen3-coder-next-200k:latest").await;
        assert_eq!(n, 262_144);
    }

    #[tokio::test]
    async fn falls_back_when_no_profile() {
        let repo = StubRepo { entry: None };
        let n = resolve_model_context_size(&repo, Uuid::nil(), "unknown-model").await;
        assert_eq!(n, 32_768);
    }

    #[tokio::test]
    async fn falls_back_when_below_sanity_floor() {
        // configured_ctx=2048 is below the 4096 sanity floor — treated as corrupt
        // and replaced with the 32_768 legacy default.
        let repo = StubRepo {
            entry: Some(make_entry(2048)),
        };
        let n = resolve_model_context_size(&repo, Uuid::nil(), "broken-row").await;
        assert_eq!(n, 32_768);
    }

    #[tokio::test]
    async fn falls_back_when_negative() {
        // Defense in depth — `.configured_ctx` is `i32`; clamp negatives via `.max(0)`.
        let repo = StubRepo {
            entry: Some(make_entry(-1)),
        };
        let n = resolve_model_context_size(&repo, Uuid::nil(), "negative-row").await;
        assert_eq!(n, 32_768);
    }
}
