use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ModelCapacityEntry {
    pub backend_id:            Uuid,
    pub model_name:            String,
    // VRAM
    pub vram_model_mb:         i32,
    pub vram_total_mb:         i32,
    // Architecture from /api/show
    pub arch_num_layers:       i32,
    pub arch_num_kv_heads:     i32,
    pub arch_head_dim:         i32,
    pub arch_configured_ctx:   i32,
    // KV cache calculation
    pub vram_kv_per_slot_mb:   i32,   // realistic (avg_tokens basis)
    pub vram_kv_worst_case_mb: i32,   // worst case (num_ctx basis)
    // Recommended concurrency
    pub recommended_slots:     i16,
    // Throughput stats
    pub avg_tokens_per_sec:    f64,
    pub avg_prefill_tps:       f64,
    pub avg_prompt_tokens:     f64,
    pub avg_output_tokens:     f64,
    pub p95_latency_ms:        f64,
    pub sample_count:          i32,
    // LLM analysis
    pub llm_concern:           Option<String>,
    pub llm_reason:            Option<String>,
    pub updated_at:            DateTime<Utc>,
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
    async fn upsert(&self, entry: &ModelCapacityEntry) -> Result<()>;
    async fn get(&self, backend_id: Uuid, model: &str) -> Result<Option<ModelCapacityEntry>>;
    async fn list_all(&self) -> Result<Vec<ModelCapacityEntry>>;
    /// Aggregate throughput stats from completed inference_jobs over the last `window_hours`.
    async fn compute_throughput_stats(
        &self,
        backend_id: Uuid,
        model: &str,
        window_hours: u32,
    ) -> Result<Option<ThroughputStats>>;
}
