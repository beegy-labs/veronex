use anyhow::Result;
use async_trait::async_trait;

/// Outbound port for GPU model lifecycle management.
///
/// Tracks which models are loaded on each provider, enforces an LRU eviction
/// policy, and proactively unloads stale models so that the requested model
/// can be loaded without running out of memory.
///
/// Only Ollama providers require active management; Gemini is API-based and
/// has no local model state.
#[async_trait]
pub trait ModelManagerPort: Send + Sync {
    /// Ensure `model_name` is ready to serve inference on this manager's provider.
    ///
    /// - If the model is already loaded, updates the LRU order and returns.
    /// - If a different model is loaded and `max_loaded` would be exceeded,
    ///   evicts the least-recently-used model first.
    /// - Ollama auto-loads the target model when the first inference request
    ///   arrives, so this call only needs to handle explicit eviction.
    async fn ensure_loaded(&self, model_name: &str) -> Result<()>;

    /// Record that `model_name` was just used, bumping it to the MRU position.
    async fn record_used(&self, model_name: &str);

    /// Return loaded model names, most-recently-used first.
    async fn loaded_models(&self) -> Vec<String>;
}
