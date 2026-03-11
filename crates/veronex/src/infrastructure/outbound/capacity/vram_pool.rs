use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::{
    ModelVramProfile, VramPermit, VramPoolPort,
};

/// Per-model state within a provider's VRAM pool.
struct ModelState {
    weight_mb: u32,
    is_loaded: bool,
    kv_per_request_mb: u32,
    /// Active KV cache reservations (in MB) for this model.
    active_kv_mb: Arc<AtomicU32>,
    /// Number of active requests (for reporting).
    active_count: Arc<AtomicU32>,
    /// Adaptive concurrency limit (0 = unlimited).
    max_concurrent: AtomicU32,
    /// Baseline throughput (tps × 100) for AIMD algorithm.
    baseline_tps: AtomicU32,
    /// Baseline p95 latency (ms) for AIMD algorithm.
    baseline_p95_ms: AtomicU32,
    /// Counter for probe-slot scheduling (incremented on limit hits).
    probe_counter: AtomicU32,
}

/// Per-provider VRAM state.
struct ProviderVramState {
    total_mb: AtomicU32,
    /// Global KV reservation counter across all models.
    reserved_kv_mb: Arc<AtomicU32>,
    /// Safety buffer (in permil, e.g. 200 = 20%). Increases on OOM.
    safety_permil: AtomicU32,
    /// Model name → model state.
    models: DashMap<String, ModelState>,
}

/// Default VRAM buffer reserved for system/driver overhead (MB).
const DEFAULT_BUFFER_MB: u32 = 512;
/// Default safety margin (permil). 100 = 10%.
const DEFAULT_SAFETY_PERMIL: u32 = 100;
/// Safety margin increase on OOM (permil). 50 = 5%.
const OOM_SAFETY_BUMP_PERMIL: u32 = 50;

/// Maps provider_id → ProviderVramState.
///
/// Global VRAM pool: any model combination fits as long as total VRAM allows.
/// When total_vram == 0 (not probed), always allows requests (delegates to Ollama).
#[derive(Clone)]
pub struct VramPool {
    providers: Arc<DashMap<Uuid, Arc<ProviderVramState>>>,
    /// AIMD probe: extra (+) or fewer (-) concurrent requests for learning.
    probe_permits: Arc<AtomicI32>,
    /// AIMD probe: every N arrivals at the limit, apply probe. 0 = disabled.
    probe_rate: Arc<AtomicU32>,
}

impl VramPool {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(DashMap::new()),
            probe_permits: Arc::new(AtomicI32::new(1)),
            probe_rate: Arc::new(AtomicU32::new(3)),
        }
    }

    fn get_or_create(&self, provider_id: Uuid) -> Arc<ProviderVramState> {
        self.providers
            .entry(provider_id)
            .or_insert_with(|| {
                Arc::new(ProviderVramState {
                    total_mb: AtomicU32::new(0),
                    reserved_kv_mb: Arc::new(AtomicU32::new(0)),
                    safety_permil: AtomicU32::new(DEFAULT_SAFETY_PERMIL),
                    models: DashMap::new(),
                })
            })
            .value()
            .clone()
    }

    /// Compute total weight of loaded models.
    fn loaded_weight_mb(state: &ProviderVramState) -> u32 {
        state
            .models
            .iter()
            .filter(|e| e.is_loaded)
            .map(|e| e.weight_mb)
            .sum()
    }

    /// Check adaptive concurrency limit with probe policy.
    /// Returns true if the request should be BLOCKED.
    fn should_block(ms: &ModelState, probe_permits: i32, probe_rate: u32) -> bool {
        let limit = ms.max_concurrent.load(Ordering::Acquire);
        if limit == 0 {
            return false; // unlimited
        }
        let active = ms.active_count.load(Ordering::Acquire);
        if probe_permits > 0 {
            // Probe UP: periodically allow above limit
            let effective = limit.saturating_add(probe_permits as u32);
            if active >= effective {
                return true; // hard cap
            }
            if active >= limit {
                // In probe zone — only allow every Nth attempt
                if probe_rate == 0 {
                    return true;
                }
                let count = ms.probe_counter.fetch_add(1, Ordering::AcqRel);
                return !count.is_multiple_of(probe_rate);
            }
            false
        } else if probe_permits < 0 {
            // Probe DOWN: periodically enforce tighter limit
            if active >= limit {
                return true;
            }
            let effective = (limit as i64 + probe_permits as i64).max(1) as u32;
            if active >= effective && probe_rate > 0 {
                let count = ms.probe_counter.fetch_add(1, Ordering::AcqRel);
                if count.is_multiple_of(probe_rate) {
                    return true; // block this one to test lower concurrency
                }
            }
            false
        } else {
            // No probing — strict limit
            active >= limit
        }
    }

    /// Compute available VRAM considering loaded weights, active KV, buffer, and safety margin.
    fn compute_available(state: &ProviderVramState) -> i64 {
        let total = state.total_mb.load(Ordering::Acquire) as i64;
        if total == 0 {
            return i64::MAX; // Not probed → unlimited
        }
        let loaded = Self::loaded_weight_mb(state) as i64;
        let kv = state.reserved_kv_mb.load(Ordering::Acquire) as i64;
        let safety = total * state.safety_permil.load(Ordering::Acquire) as i64 / 1000;
        total - loaded - kv - DEFAULT_BUFFER_MB as i64 - safety
    }
}

impl Default for VramPool {
    fn default() -> Self {
        Self::new()
    }
}

impl VramPoolPort for VramPool {
    fn try_reserve(&self, provider_id: Uuid, model: &str) -> Option<VramPermit> {
        let state = self.get_or_create(provider_id);
        let total = state.total_mb.load(Ordering::Acquire);

        // If total VRAM is 0 (not probed), always allow — delegate capacity to Ollama.
        if total == 0 {
            // Create a zero-cost permit that tracks request count only.
            // Cold start: new models default to max_concurrent=1 until learned.
            let model_state = state.models.entry(model.to_string()).or_insert_with(|| {
                ModelState {
                    weight_mb: 0,
                    is_loaded: false,
                    kv_per_request_mb: 0,
                    active_kv_mb: Arc::new(AtomicU32::new(0)),
                    active_count: Arc::new(AtomicU32::new(0)),
                    max_concurrent: AtomicU32::new(1),
                    baseline_tps: AtomicU32::new(0),
                    baseline_p95_ms: AtomicU32::new(0),
                    probe_counter: AtomicU32::new(0),
                }
            });
            // Adaptive concurrency check (even when VRAM is not probed).
            let pp = self.probe_permits.load(Ordering::Acquire);
            let pr = self.probe_rate.load(Ordering::Acquire);
            if Self::should_block(&model_state, pp, pr) {
                return None;
            }
            model_state.active_count.fetch_add(1, Ordering::AcqRel);
            let active_count = model_state.active_count.clone();
            let reserved_kv = state.reserved_kv_mb.clone();
            return Some(VramPermit::new(0, reserved_kv, active_count));
        }

        let model_entry = state.models.get(model);
        let (kv_mb, need_load_weight) = match model_entry {
            Some(ref ms) if ms.is_loaded => {
                // Model already loaded: only need KV cache.
                (ms.kv_per_request_mb.max(32), false) // minimum 32MB KV
            }
            Some(ref ms) => {
                // Model known but not loaded: need weight + KV.
                let _ = ms; // weight will be accounted below
                let kv = ms.kv_per_request_mb.max(32);
                (kv, true)
            }
            None => {
                // Unknown model: conservative estimate.
                (128, true)
            }
        };

        // Adaptive concurrency check before VRAM reservation.
        if let Some(ref ms) = model_entry {
            let pp = self.probe_permits.load(Ordering::Acquire);
            let pr = self.probe_rate.load(Ordering::Acquire);
            if Self::should_block(ms, pp, pr) {
                return None;
            }
        }

        let weight_cost = if need_load_weight {
            model_entry
                .as_ref()
                .map(|ms| ms.weight_mb)
                .unwrap_or(2048) // conservative 2GB estimate for unknown models
        } else {
            0
        };
        drop(model_entry);

        // CAS loop: atomically reserve KV cache.
        const MAX_CAS_RETRIES: u32 = 16;
        let reserved_kv = state.reserved_kv_mb.clone();
        for _ in 0..MAX_CAS_RETRIES {
            let cur_kv = reserved_kv.load(Ordering::Acquire);
            let loaded = Self::loaded_weight_mb(&state) as i64;
            let safety = total as i64 * state.safety_permil.load(Ordering::Acquire) as i64 / 1000;

            // APU / unified-memory case: if loaded models already exceed the DRM-reported
            // total VRAM, the hardware is using shared system RAM (e.g. AMD Ryzen AI 395+).
            // Trust the concurrency limit instead of VRAM arithmetic, which would incorrectly
            // block all requests when loaded_weight > total_vram.
            let available = if loaded + weight_cost as i64 > total as i64 {
                i64::MAX
            } else {
                total as i64 - loaded - cur_kv as i64 - DEFAULT_BUFFER_MB as i64 - safety - weight_cost as i64
            };

            if available < kv_mb as i64 {
                // Not enough VRAM — bump safety factor for next time.
                let cur_safety = state.safety_permil.load(Ordering::Acquire);
                let new_safety = (cur_safety + OOM_SAFETY_BUMP_PERMIL).min(300);
                state.safety_permil.store(new_safety, Ordering::Release);
                return None;
            }

            if reserved_kv
                .compare_exchange(cur_kv, cur_kv + kv_mb, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // Mark model as loaded if we're loading it.
                if need_load_weight {
                    state
                        .models
                        .entry(model.to_string())
                        .and_modify(|ms| {
                            ms.is_loaded = true;
                        })
                        .or_insert_with(|| ModelState {
                            weight_mb: weight_cost,
                            is_loaded: true,
                            kv_per_request_mb: kv_mb,
                            active_kv_mb: Arc::new(AtomicU32::new(0)),
                            active_count: Arc::new(AtomicU32::new(0)),
                            max_concurrent: AtomicU32::new(1),
                            baseline_tps: AtomicU32::new(0),
                            baseline_p95_ms: AtomicU32::new(0),
                            probe_counter: AtomicU32::new(0),
                        });
                }

                // Track per-model active KV and count.
                let model_state = state.models.entry(model.to_string()).or_insert_with(|| {
                    ModelState {
                        weight_mb: weight_cost,
                        is_loaded: need_load_weight,
                        kv_per_request_mb: kv_mb,
                        active_kv_mb: Arc::new(AtomicU32::new(0)),
                        active_count: Arc::new(AtomicU32::new(0)),
                        max_concurrent: AtomicU32::new(1),
                        baseline_tps: AtomicU32::new(0),
                        baseline_p95_ms: AtomicU32::new(0),
                        probe_counter: AtomicU32::new(0),
                    }
                });
                model_state.active_kv_mb.fetch_add(kv_mb, Ordering::AcqRel);
                model_state.active_count.fetch_add(1, Ordering::AcqRel);
                let active_count = model_state.active_count.clone();

                return Some(VramPermit::new(kv_mb, reserved_kv, active_count));
            }
            // CAS failed — another thread won the race; retry.
        }
        tracing::warn!(provider_id = %provider_id, model = %model, "VRAM CAS retries exhausted");
        None
    }

    fn total_vram_mb(&self, provider_id: Uuid) -> u32 {
        self.providers
            .get(&provider_id)
            .map(|s| s.total_mb.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    fn used_vram_mb(&self, provider_id: Uuid) -> u32 {
        self.providers
            .get(&provider_id)
            .map(|s| {
                let loaded = Self::loaded_weight_mb(&s);
                let kv = s.reserved_kv_mb.load(Ordering::Acquire);
                loaded + kv
            })
            .unwrap_or(0)
    }

    fn available_vram_mb(&self, provider_id: Uuid) -> u32 {
        self.providers
            .get(&provider_id)
            .map(|s| Self::compute_available(&s).max(0) as u32)
            .unwrap_or(0)
    }

    fn set_total_vram(&self, provider_id: Uuid, total_mb: u32) {
        let state = self.get_or_create(provider_id);
        state.total_mb.store(total_mb, Ordering::Release);
    }

    fn set_model_profile(&self, provider_id: Uuid, model: &str, profile: ModelVramProfile) {
        let state = self.get_or_create(provider_id);
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| {
                ms.weight_mb = profile.weight_mb;
                ms.kv_per_request_mb = profile.kv_per_request_mb;
            })
            .or_insert_with(|| ModelState {
                weight_mb: profile.weight_mb,
                is_loaded: false,
                kv_per_request_mb: profile.kv_per_request_mb,
                active_kv_mb: Arc::new(AtomicU32::new(0)),
                active_count: Arc::new(AtomicU32::new(0)),
                max_concurrent: AtomicU32::new(0),
                baseline_tps: AtomicU32::new(0),
                baseline_p95_ms: AtomicU32::new(0),
                probe_counter: AtomicU32::new(0),
            });
    }

    fn mark_model_loaded(&self, provider_id: Uuid, model: &str, weight_mb: u32) {
        let state = self.get_or_create(provider_id);
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| {
                ms.is_loaded = true;
                ms.weight_mb = weight_mb;
            })
            .or_insert_with(|| ModelState {
                weight_mb,
                is_loaded: true,
                kv_per_request_mb: 128,
                active_kv_mb: Arc::new(AtomicU32::new(0)),
                active_count: Arc::new(AtomicU32::new(0)),
                max_concurrent: AtomicU32::new(0),
                baseline_tps: AtomicU32::new(0),
                baseline_p95_ms: AtomicU32::new(0),
                probe_counter: AtomicU32::new(0),
            });
    }

    fn mark_model_unloaded(&self, provider_id: Uuid, model: &str) {
        let state = self.get_or_create(provider_id);
        if let Some(mut ms) = state.models.get_mut(model) {
            ms.is_loaded = false;
        }
    }

    fn active_requests(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers
            .get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.active_count.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn provider_active_requests(&self, provider_id: Uuid) -> u32 {
        self.providers
            .get(&provider_id)
            .map(|s| s.models.iter().map(|ms| ms.active_count.load(Ordering::Acquire)).sum())
            .unwrap_or(0)
    }

    fn loaded_model_names(&self, provider_id: Uuid) -> Vec<String> {
        self.providers
            .get(&provider_id)
            .map(|s| {
                s.models
                    .iter()
                    .filter(|e| e.is_loaded)
                    .map(|e| e.key().clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_max_concurrent(&self, provider_id: Uuid, model: &str, limit: u32) {
        let state = self.get_or_create(provider_id);
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| { ms.max_concurrent.store(limit, Ordering::Release); })
            .or_insert_with(|| ModelState {
                weight_mb: 0,
                is_loaded: false,
                kv_per_request_mb: 128,
                active_kv_mb: Arc::new(AtomicU32::new(0)),
                active_count: Arc::new(AtomicU32::new(0)),
                max_concurrent: AtomicU32::new(limit),
                baseline_tps: AtomicU32::new(0),
                baseline_p95_ms: AtomicU32::new(0),
                probe_counter: AtomicU32::new(0),
            });
    }

    fn max_concurrent(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers
            .get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.max_concurrent.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn set_baseline_tps(&self, provider_id: Uuid, model: &str, tps_x100: u32) {
        let state = self.get_or_create(provider_id);
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| { ms.baseline_tps.store(tps_x100, Ordering::Release); })
            .or_insert_with(|| ModelState {
                weight_mb: 0,
                is_loaded: false,
                kv_per_request_mb: 128,
                active_kv_mb: Arc::new(AtomicU32::new(0)),
                active_count: Arc::new(AtomicU32::new(0)),
                max_concurrent: AtomicU32::new(1),
                baseline_tps: AtomicU32::new(tps_x100),
                baseline_p95_ms: AtomicU32::new(0),
                probe_counter: AtomicU32::new(0),
            });
    }

    fn baseline_tps(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers
            .get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.baseline_tps.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn set_baseline_p95_ms(&self, provider_id: Uuid, model: &str, p95_ms: u32) {
        let state = self.get_or_create(provider_id);
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| { ms.baseline_p95_ms.store(p95_ms, Ordering::Release); })
            .or_insert_with(|| ModelState {
                weight_mb: 0,
                is_loaded: false,
                kv_per_request_mb: 128,
                active_kv_mb: Arc::new(AtomicU32::new(0)),
                active_count: Arc::new(AtomicU32::new(0)),
                max_concurrent: AtomicU32::new(1),
                baseline_tps: AtomicU32::new(0),
                baseline_p95_ms: AtomicU32::new(p95_ms),
                probe_counter: AtomicU32::new(0),
            });
    }

    fn baseline_p95_ms(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers
            .get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.baseline_p95_ms.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn set_probe_config(&self, permits: i32, rate: i32) {
        self.probe_permits.store(permits, Ordering::Release);
        self.probe_rate.store(rate.max(0) as u32, Ordering::Release);
    }
}
