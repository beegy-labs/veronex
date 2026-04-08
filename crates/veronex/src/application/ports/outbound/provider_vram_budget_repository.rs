use anyhow::Result;
use uuid::Uuid;

/// Persistent VRAM budget state for an Ollama provider.
///
/// Stored in `provider_vram_budget`. Complements `llm_providers.num_parallel`
/// and `llm_providers.total_vram_mb` (which are managed via the provider API).
#[derive(Debug, Clone)]
pub struct ProviderVramBudget {
    pub provider_id: Uuid,
    /// Safety margin in permil (100 = 10%). Increases on OOM (+50), decreases on stable (-10).
    pub safety_permil: i32,
    /// How vram_total_mb was learned: "probe" | "node_exporter" | "manual"
    pub vram_total_source: String,
    /// KV cache quantization type: "f16" | "q8_0" | "q4_0"
    pub kv_cache_type: String,
}


#[async_trait::async_trait]
pub trait ProviderVramBudgetRepository: Send + Sync {
    async fn get(&self, provider_id: Uuid) -> Result<Option<ProviderVramBudget>>;
    async fn upsert(&self, budget: &ProviderVramBudget) -> Result<()>;
}
