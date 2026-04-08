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
}
