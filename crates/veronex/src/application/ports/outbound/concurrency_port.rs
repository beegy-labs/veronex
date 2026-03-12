use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use uuid::Uuid;

/// VRAM profile for a model — architecture-derived KV cache estimation.
#[derive(Debug, Clone)]
pub struct ModelVramProfile {
    pub weight_mb: u32,
    pub weight_estimated: bool,
    pub kv_per_request_mb: u32,
    pub num_layers: u16,
    pub num_kv_heads: u16,
    pub head_dim: u16,
    pub configured_ctx: u32,
    pub failure_count: u16,
    pub llm_concern: Option<String>,
    pub llm_reason: Option<String>,
}

/// RAII VRAM permit — releases KV cache reservation on drop.
///
/// Weight stays loaded (models persist in VRAM between requests).
/// Only the per-request KV cache allocation is returned on drop.
pub struct VramPermit {
    kv_mb: u32,
    reserved_kv: Option<Arc<AtomicU32>>,
    active_count: Option<Arc<AtomicU32>>,
    release_tx: Option<tokio::sync::oneshot::Sender<u32>>,
    /// Updated to current Unix ms on drop (Phase 7: last_active_at tracking).
    last_active_at: Option<Arc<AtomicU64>>,
}

impl VramPermit {
    /// Create a local (in-process) permit backed by an atomic counter.
    pub(crate) fn new(
        kv_mb: u32,
        reserved_kv: Arc<AtomicU32>,
        active_count: Arc<AtomicU32>,
    ) -> Self {
        Self { kv_mb, reserved_kv: Some(reserved_kv), active_count: Some(active_count), release_tx: None, last_active_at: None }
    }

    /// Create a local permit with last_active_at tracking (Phase 7).
    pub(crate) fn with_last_active(
        kv_mb: u32,
        reserved_kv: Arc<AtomicU32>,
        active_count: Arc<AtomicU32>,
        last_active_at: Arc<AtomicU64>,
    ) -> Self {
        Self { kv_mb, reserved_kv: Some(reserved_kv), active_count: Some(active_count), release_tx: None, last_active_at: Some(last_active_at) }
    }

    /// Create a combined permit: local atomic decrement + distributed Valkey release.
    pub(crate) fn combined(
        kv_mb: u32,
        reserved_kv: Arc<AtomicU32>,
        active_count: Arc<AtomicU32>,
        release_tx: tokio::sync::oneshot::Sender<u32>,
    ) -> Self {
        Self { kv_mb, reserved_kv: Some(reserved_kv), active_count: Some(active_count), release_tx: Some(release_tx), last_active_at: None }
    }

    /// Extract internals, consuming this permit without triggering drop.
    pub(crate) fn into_parts(mut self) -> Option<(Arc<AtomicU32>, Arc<AtomicU32>, u32)> {
        let reserved = self.reserved_kv.take();
        let active = self.active_count.take();
        let kv = self.kv_mb;
        self.release_tx = None;
        self.last_active_at = None;
        std::mem::forget(self);
        reserved.zip(active).map(|(r, a)| (r, a, kv))
    }
}

impl Drop for VramPermit {
    fn drop(&mut self) {
        if let Some(ref reserved_kv) = self.reserved_kv {
            reserved_kv.fetch_sub(self.kv_mb, Ordering::Release);
        }
        if let Some(ref active) = self.active_count {
            active.fetch_sub(1, Ordering::Release);
        }
        if let Some(ref last_active) = self.last_active_at {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            last_active.store(now_ms, Ordering::Release);
        }
        if let Some(tx) = self.release_tx.take() {
            let _ = tx.send(self.kv_mb);
        }
    }
}

/// Port for per-provider VRAM pool management.
///
/// Replaces the old per-(provider, model) slot-based concurrency control with
/// a global VRAM pool per provider: any model combination fits as long as VRAM allows.
pub trait VramPoolPort: Send + Sync {
    /// Try to reserve VRAM for a request on the given provider + model.
    ///
    /// - If model is loaded: only reserves KV cache.
    /// - If model is NOT loaded: reserves weight + KV cache.
    /// - If total_vram == 0 (not yet probed): always allows (delegates to Ollama).
    fn try_reserve(&self, provider_id: Uuid, model: &str) -> Option<VramPermit>;

    /// Total VRAM for a provider (0 = not yet probed).
    fn total_vram_mb(&self, provider_id: Uuid) -> u32;

    /// Currently used VRAM (loaded model weights + active KV cache).
    fn used_vram_mb(&self, provider_id: Uuid) -> u32;

    /// Available VRAM for new allocations.
    fn available_vram_mb(&self, provider_id: Uuid) -> u32;

    /// Set the total VRAM for a provider (from hw_metrics probe).
    fn set_total_vram(&self, provider_id: Uuid, total_mb: u32);

    /// Register or update the VRAM profile for a model.
    fn set_model_profile(&self, provider_id: Uuid, model: &str, profile: ModelVramProfile);

    /// Mark a model as loaded (its weight occupies VRAM).
    fn mark_model_loaded(&self, provider_id: Uuid, model: &str, weight_mb: u32);

    /// Mark a model as unloaded (its weight is freed).
    fn mark_model_unloaded(&self, provider_id: Uuid, model: &str);

    /// Number of active requests for a specific model on a provider.
    fn active_requests(&self, provider_id: Uuid, model: &str) -> u32;

    /// Total active requests across all models on a provider.
    fn provider_active_requests(&self, provider_id: Uuid) -> u32;

    /// List model names currently marked as loaded for a provider.
    fn loaded_model_names(&self, provider_id: Uuid) -> Vec<String>;

    /// Update adaptive concurrency limit for a model. 0 = unlimited.
    fn set_max_concurrent(&self, provider_id: Uuid, model: &str, limit: u32);

    /// Get current max concurrent limit for a model. 0 = unlimited.
    fn max_concurrent(&self, provider_id: Uuid, model: &str) -> u32;

    /// Update baseline throughput for AIMD algorithm (tps × 100, stored as integer).
    fn set_baseline_tps(&self, provider_id: Uuid, model: &str, tps_x100: u32);

    /// Get baseline throughput (tps × 100).
    fn baseline_tps(&self, provider_id: Uuid, model: &str) -> u32;

    /// Check if a model is loaded on any provider.
    fn is_model_loaded(&self, model: &str) -> bool;

    /// Update baseline p95 latency (ms) for AIMD algorithm.
    fn set_baseline_p95_ms(&self, provider_id: Uuid, model: &str, p95_ms: u32);

    /// Get baseline p95 latency (ms).
    fn baseline_p95_ms(&self, provider_id: Uuid, model: &str) -> u32;

    /// Update probe config from capacity_settings.
    fn set_probe_config(&self, permits: i32, rate: i32);

    // ── Phase 7: model state fields ─────────────────────────────────────

    /// Check if a model is currently being preloaded on any provider.
    fn is_preloading(&self, provider_id: Uuid, model: &str) -> bool;

    /// Set preloading state for a model+provider pair.
    fn set_preloading(&self, provider_id: Uuid, model: &str, value: bool);

    /// Get consecutive preload failure count.
    fn preload_fail_count(&self, provider_id: Uuid, model: &str) -> u32;

    /// Record a preload failure (increment count, trigger 300s exclusion at 3).
    fn record_preload_failure(&self, provider_id: Uuid, model: &str);

    /// Record a preload success (reset fail count + failed_at).
    fn record_preload_success(&self, provider_id: Uuid, model: &str);

    /// Unix ms when 3-consecutive preload failure triggered exclusion (0 = normal).
    fn preload_failed_at(&self, provider_id: Uuid, model: &str) -> u64;

    /// Check if model+provider is excluded due to preload failures (within 300s window).
    fn is_preload_excluded(&self, provider_id: Uuid, model: &str) -> bool;

    /// Check if a model pull is in progress.
    fn is_pulling(&self, provider_id: Uuid, model: &str) -> bool;

    /// Set pulling state for a model+provider pair.
    fn set_pulling(&self, provider_id: Uuid, model: &str, value: bool);

    /// Get dispatch_blocked flag (governor share=0).
    fn is_dispatch_blocked(&self, provider_id: Uuid, model: &str) -> bool;

    /// Set dispatch_blocked flag.
    fn set_dispatch_blocked(&self, provider_id: Uuid, model: &str, value: bool);

    /// Get pre-Hard max_concurrent snapshot.
    fn pre_hard_max_concurrent(&self, provider_id: Uuid, model: &str) -> u32;

    /// Store pre-Hard max_concurrent snapshot.
    fn set_pre_hard_max_concurrent(&self, provider_id: Uuid, model: &str, value: u32);

    /// Seconds since last active request for a model+provider.
    fn idle_since_secs(&self, provider_id: Uuid, model: &str) -> u64;
}
