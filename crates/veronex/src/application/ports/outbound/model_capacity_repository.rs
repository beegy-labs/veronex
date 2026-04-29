use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ModelVramProfileEntry {
    pub provider_id:       Uuid,
    pub model_name:        String,
    pub weight_mb:         i32,
    pub weight_estimated:  bool,
    pub kv_per_request_mb: i32,
    pub num_layers:        i16,
    pub num_kv_heads:      i16,
    pub head_dim:          i16,
    pub configured_ctx:    i32,
    pub max_ctx:           i32,
    pub failure_count:     i16,
    pub llm_concern:       Option<String>,
    pub llm_reason:        Option<String>,
    pub max_concurrent:    i32,
    pub baseline_tps:      i32,
    pub baseline_p95_ms:   i32,
    pub updated_at:        DateTime<Utc>,
}

/// Throughput statistics aggregated from inference_jobs over a time window.
#[derive(Debug, Default, Clone)]
pub struct ThroughputStats {
    pub avg_tokens_per_sec: f64,
    pub avg_prefill_tps:    f64,
    pub avg_prompt_tokens:  f64,
    pub avg_output_tokens:  f64,
    pub p95_latency_ms:     f64,
    pub sample_count:       i64,
}

#[async_trait]
pub trait ModelCapacityRepository: Send + Sync {
    async fn upsert(&self, entry: &ModelVramProfileEntry) -> Result<()>;
    async fn get(&self, provider_id: Uuid, model: &str) -> Result<Option<ModelVramProfileEntry>>;
    async fn list_all(&self) -> Result<Vec<ModelVramProfileEntry>>;
    async fn list_by_provider(&self, provider_id: Uuid) -> Result<Vec<ModelVramProfileEntry>>;
    /// Fetch entries for a batch of providers in a single query.
    async fn list_by_providers(&self, ids: &[Uuid]) -> Result<Vec<ModelVramProfileEntry>>;
    /// Aggregate throughput stats from completed inference_jobs over the last `window_hours`.
    async fn compute_throughput_stats(
        &self,
        provider_id: Uuid,
        model: &str,
        window_hours: u32,
    ) -> Result<Option<ThroughputStats>>;

    /// Returns true if any selected provider/model pair has no `model_vram_profiles`
    /// row yet. The capacity analyzer's demand gate uses this to bypass idle-skip
    /// when a freshly-selected model still needs an initial probe.
    /// SDD: `.specs/veronex/history/inference-mcp-per-round-persist.md` §6.
    async fn has_unprofiled_selected_models(&self) -> Result<bool>;

    /// Returns the minimum `configured_ctx` across all `model_vram_profiles`
    /// rows for `model`. Used by the context-budget pre-flight (S17) at
    /// request entry, before the dispatcher has chosen a provider — the
    /// budget must fit even on the smallest-ctx provider that may be
    /// selected. Filters values below the 4096 sanity floor; `None` when
    /// no row exists.
    /// SDD: `.specs/veronex/conversation-context-compression.md` §3.
    async fn min_configured_ctx_for_model(&self, model: &str) -> Result<Option<u32>>;
}
