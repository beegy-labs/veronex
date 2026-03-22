use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::{
    ModelVramProfile, VramPermit, VramPoolPort,
};

/// Per-model state within a provider's VRAM pool.
///
/// Scope: model × provider pair. All `Atomic*` fields are concurrency-safe.
struct ModelState {
    weight_mb: u64,
    is_loaded: bool,
    kv_per_request_mb: u64,
    /// Active KV cache reservations (in MB) for this model.
    active_kv_mb: Arc<AtomicU64>,
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

    // ── Phase 7 fields ──────────────────────────────────────────────────

    /// Unix ms of last request completion (updated on VramPermit::drop).
    last_active_at: Arc<AtomicU64>,
    /// Preloader currently running for this model+provider.
    is_preloading: AtomicBool,
    /// Pull (download) in progress for this model+provider.
    is_pulling: AtomicBool,
    /// AIMD sample count (reset on evict).
    sample_count: AtomicU32,
    /// Consecutive preload failure count (reset on success).
    preload_fail_count: AtomicU32,
    /// Unix ms when 3-consecutive preload failure started (0 = normal).
    /// filter_candidates excludes model+provider when `now - preload_failed_at < 300_000ms`.
    preload_failed_at: AtomicU64,
    /// Epoch start for ClickHouse aggregation (updated on evict).
    learning_epoch_started_at: AtomicU64,
    /// Governor share=0 dispatch block flag (avoids max_concurrent=0 deadlock).
    dispatch_blocked: AtomicBool,
    /// max_concurrent snapshot before Hard throttle (for RampUp exit condition).
    pre_hard_max_concurrent: AtomicU32,
    /// Governor dispatch cap for this AIMD cycle (0 = no governor cap active).
    governor_cap: AtomicU32,
    /// Consecutive stable cycles for 3-cycle AIMD baseline update (reset on decrease).
    stable_cycle_count: AtomicU32,
}

impl ModelState {
    fn new(weight_mb: u64, is_loaded: bool, kv_per_request_mb: u64, max_concurrent: u32) -> Self {
        Self {
            weight_mb,
            is_loaded,
            kv_per_request_mb,
            active_kv_mb: Arc::new(AtomicU64::new(0)),
            active_count: Arc::new(AtomicU32::new(0)),
            max_concurrent: AtomicU32::new(max_concurrent),
            baseline_tps: AtomicU32::new(0),
            baseline_p95_ms: AtomicU32::new(0),
            probe_counter: AtomicU32::new(0),
            last_active_at: Arc::new(AtomicU64::new(0)),
            is_preloading: AtomicBool::new(false),
            is_pulling: AtomicBool::new(false),
            sample_count: AtomicU32::new(0),
            preload_fail_count: AtomicU32::new(0),
            preload_failed_at: AtomicU64::new(0),
            learning_epoch_started_at: AtomicU64::new(0),
            dispatch_blocked: AtomicBool::new(false),
            pre_hard_max_concurrent: AtomicU32::new(0),
            governor_cap: AtomicU32::new(0),
            stable_cycle_count: AtomicU32::new(0),
        }
    }
}

/// Per-provider VRAM state.
struct ProviderVramState {
    total_mb: AtomicU64,
    /// Global KV reservation counter across all models.
    reserved_kv_mb: Arc<AtomicU64>,
    /// Safety buffer (in permil, e.g. 200 = 20%). Increases on OOM.
    safety_permil: AtomicU32,
    /// Model name → model state.
    models: DashMap<String, ModelState>,
    /// Cached sum of weight_mb for all currently-loaded models (O(1) reads).
    /// Updated atomically on mark_model_loaded / mark_model_unloaded.
    cached_loaded_weight_mb: AtomicU64,

    // ── Phase 7 fields ──────────────────────────────────────────────────

    /// Provider is in standby mode (no active models expected).
    is_standby: AtomicBool,
    /// Unix ms until which a state transition is in progress.
    transition_until: AtomicU64,
    /// Last observed mem_available_mb (APU drift detection). 0 = not yet set.
    last_mem_available_mb: AtomicU32,
    /// Cached total active request count across all models — O(1) alternative to summing models.
    total_active_count: Arc<AtomicU32>,
    /// Cached max(max_concurrent) across all loaded models for this provider.
    /// Used by available_vram_mb APU path to avoid O(models) scan on every scoring call.
    apu_max_concurrent_cache: AtomicU32,
}

/// Default VRAM buffer reserved for system/driver overhead (MB).
const DEFAULT_BUFFER_MB: u32 = 512;
/// Default safety margin (permil). 100 = 10%.
const DEFAULT_SAFETY_PERMIL: u32 = 100;
/// Safety margin increase on OOM (permil). 50 = 5%.
const OOM_SAFETY_BUMP_PERMIL: u32 = 50;
/// Safety margin decay per stable APU sync cycle (permil). 10 = 1%.
const SAFETY_DECAY_PERMIL: u32 = 10;

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
    /// Global set of model names currently loaded on any provider — O(1) cross-provider lookup.
    loaded_models_global: Arc<DashSet<String>>,
    /// Refcount per model: how many providers currently have it loaded.
    /// O(1) update on load/unload; replaces O(providers) scan in mark_model_unloaded.
    loaded_model_refcounts: Arc<DashMap<String, AtomicU64>>,
}

impl VramPool {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(DashMap::new()),
            probe_permits: Arc::new(AtomicI32::new(1)),
            probe_rate: Arc::new(AtomicU32::new(3)),
            loaded_models_global: Arc::new(DashSet::new()),
            loaded_model_refcounts: Arc::new(DashMap::new()),
        }
    }

    fn get_or_create(&self, provider_id: Uuid) -> Arc<ProviderVramState> {
        self.providers
            .entry(provider_id)
            .or_insert_with(|| {
                Arc::new(ProviderVramState {
                    total_mb: AtomicU64::new(0),
                    reserved_kv_mb: Arc::new(AtomicU64::new(0)),
                    safety_permil: AtomicU32::new(DEFAULT_SAFETY_PERMIL),
                    models: DashMap::new(),
                    cached_loaded_weight_mb: AtomicU64::new(0),
                    is_standby: AtomicBool::new(false),
                    transition_until: AtomicU64::new(0),
                    last_mem_available_mb: AtomicU32::new(0),
                    total_active_count: Arc::new(AtomicU32::new(0)),
                    apu_max_concurrent_cache: AtomicU32::new(4),
                })
            })
            .value()
            .clone()
    }

    /// Total weight of loaded models — O(1) via cached_loaded_weight_mb.
    /// Updated atomically by mark_model_loaded / mark_model_unloaded.
    #[inline]
    fn loaded_weight_mb(state: &ProviderVramState) -> u64 {
        state.cached_loaded_weight_mb.load(Ordering::Acquire)
    }

    /// Check adaptive concurrency limit with probe policy.
    /// Returns true if the request should be BLOCKED.
    fn should_block(ms: &ModelState, probe_permits: i32, probe_rate: u32) -> bool {
        // Governor dispatch_blocked → always block (fair-share share=0)
        if ms.dispatch_blocked.load(Ordering::Acquire) {
            return true;
        }

        let limit = ms.max_concurrent.load(Ordering::Acquire);
        if limit == 0 {
            return false; // unlimited
        }

        // Governor cap: use min(max_concurrent, governor_cap) when cap > 0
        let gov_cap = ms.governor_cap.load(Ordering::Acquire);
        let limit = if gov_cap > 0 { limit.min(gov_cap) } else { limit };

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
                ModelState::new(0, false, 0, 1)
            });
            // Adaptive concurrency check (even when VRAM is not probed).
            let pp = self.probe_permits.load(Ordering::Acquire);
            let pr = self.probe_rate.load(Ordering::Acquire);
            if Self::should_block(&model_state, pp, pr) {
                return None;
            }
            model_state.active_count.fetch_add(1, Ordering::AcqRel);
            state.total_active_count.fetch_add(1, Ordering::AcqRel);
            let active_count = model_state.active_count.clone();
            let last_active = model_state.last_active_at.clone();
            let reserved_kv = state.reserved_kv_mb.clone();
            let prov_active = state.total_active_count.clone();
            return Some(VramPermit::with_last_active(0, reserved_kv, active_count, last_active, prov_active));
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
                let new_safety = (cur_safety + OOM_SAFETY_BUMP_PERMIL).min(500);
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
                        .or_insert_with(|| ModelState::new(weight_cost, true, kv_mb, 1));
                }

                // Track per-model active KV and count.
                let model_state = state.models.entry(model.to_string()).or_insert_with(|| {
                    ModelState::new(weight_cost, need_load_weight, kv_mb, 1)
                });
                model_state.active_kv_mb.fetch_add(kv_mb, Ordering::AcqRel);
                model_state.active_count.fetch_add(1, Ordering::AcqRel);
                state.total_active_count.fetch_add(1, Ordering::AcqRel);
                let active_count = model_state.active_count.clone();
                let last_active = model_state.last_active_at.clone();
                let prov_active = state.total_active_count.clone();

                return Some(VramPermit::with_last_active(kv_mb, reserved_kv, active_count, last_active, prov_active));
            }
            // CAS failed — another thread won the race; retry.
        }
        tracing::warn!(provider_id = %provider_id, model = %model, "VRAM CAS retries exhausted");
        None
    }

    fn total_vram_mb(&self, provider_id: Uuid) -> u64 {
        self.providers
            .get(&provider_id)
            .map(|s| s.total_mb.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    fn used_vram_mb(&self, provider_id: Uuid) -> u64 {
        self.providers
            .get(&provider_id)
            .map(|s| Self::loaded_weight_mb(&s) + s.reserved_kv_mb.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    fn available_vram_mb(&self, provider_id: Uuid) -> u64 {
        self.providers
            .get(&provider_id)
            .map(|s| {
                let raw = Self::compute_available(&s);
                if raw == i64::MAX {
                    // VRAM not probed (APU/iGPU unified memory).
                    // Return a concurrency-headroom-based score so unprobed providers
                    // compete fairly with VRAM-probed providers instead of always winning.
                    let active = s.total_active_count.load(Ordering::Acquire);
                    // Use cached max_concurrent instead of O(models) scan.
                    let mc = s.apu_max_concurrent_cache.load(Ordering::Acquire).max(4);
                    // 1,024 MB per free slot — at least 1 so it's not filtered out
                    mc.saturating_sub(active).saturating_mul(1_024).max(1) as u64
                } else {
                    raw.max(0) as u64
                }
            })
            .unwrap_or(0)
    }

    fn set_total_vram(&self, provider_id: Uuid, total_mb: u64) {
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
            .or_insert_with(|| ModelState::new(profile.weight_mb, false, profile.kv_per_request_mb, 0));
    }

    fn mark_model_loaded(&self, provider_id: Uuid, model: &str, weight_mb: u64) {
        let state = self.get_or_create(provider_id);
        // Update cached weight: subtract old weight if already loaded, add new weight.
        let prev = state.models.get(model).map(|ms| (ms.is_loaded, ms.weight_mb));
        match prev {
            Some((true, old_w)) if old_w != weight_mb => {
                // Weight changed — swap delta in cache.
                state.cached_loaded_weight_mb.fetch_add(weight_mb.saturating_sub(old_w), Ordering::AcqRel);
            }
            Some((false, _)) | None => {
                // Newly loaded — add weight to cache.
                state.cached_loaded_weight_mb.fetch_add(weight_mb, Ordering::AcqRel);
            }
            _ => {} // already loaded with same weight — no change
        }
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| {
                ms.is_loaded = true;
                ms.weight_mb = weight_mb;
                // Initialize last_active_at if never set — prevents immediate eviction
                // by placement planner (idle_since_secs() would return u64::MAX).
                if ms.last_active_at.load(Ordering::Acquire) == 0 {
                    ms.last_active_at.store(now_ms, Ordering::Release);
                }
            })
            .or_insert_with(|| {
                let ms = ModelState::new(weight_mb, true, 128, 0);
                ms.last_active_at.store(now_ms, Ordering::Release);
                ms
            });
        self.loaded_models_global.insert(model.to_string());
        // Increment refcount — O(1), replaces O(providers) scan on unload.
        self.loaded_model_refcounts
            .entry(model.to_string())
            .and_modify(|c| { c.fetch_add(1, Ordering::AcqRel); })
            .or_insert_with(|| AtomicU64::new(1));
    }

    fn mark_model_unloaded(&self, provider_id: Uuid, model: &str) {
        let state = self.get_or_create(provider_id);
        let was_loaded;
        if let Some(mut ms) = state.models.get_mut(model) {
            was_loaded = ms.is_loaded;
            if ms.is_loaded {
                // Subtract weight from cache when unloading.
                state.cached_loaded_weight_mb.fetch_sub(ms.weight_mb.min(
                    state.cached_loaded_weight_mb.load(Ordering::Acquire)
                ), Ordering::AcqRel);
            }
            ms.is_loaded = false;
            ms.sample_count.store(0, Ordering::Release);
            ms.is_preloading.store(false, Ordering::Release);
            ms.baseline_tps.store(0, Ordering::Release);
            ms.baseline_p95_ms.store(0, Ordering::Release);
            ms.stable_cycle_count.store(0, Ordering::Release);
            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            ms.learning_epoch_started_at.store(now_ms, Ordering::Release);
        } else {
            was_loaded = false;
        }
        // O(1) refcount decrement — no O(providers) scan needed.
        if was_loaded {
            let still_loaded = self
                .loaded_model_refcounts
                .get(model)
                .map(|c| c.fetch_sub(1, Ordering::AcqRel))
                .unwrap_or(0);
            // still_loaded is the value BEFORE decrement; if it was 1, now 0 → remove.
            if still_loaded <= 1 {
                self.loaded_models_global.remove(model);
                self.loaded_model_refcounts.remove(model);
            }
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
            .map(|s| s.total_active_count.load(Ordering::Acquire))
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

    fn is_model_loaded(&self, model: &str) -> bool {
        self.loaded_models_global.contains(model)
    }

    fn set_max_concurrent(&self, provider_id: Uuid, model: &str, limit: u32) {
        let state = self.get_or_create(provider_id);
        state
            .models
            .entry(model.to_string())
            .and_modify(|ms| { ms.max_concurrent.store(limit, Ordering::Release); })
            .or_insert_with(|| ModelState::new(0, false, 128, limit));
        // Update APU max_concurrent cache: take max across all models for O(1) APU scoring.
        let prev = state.apu_max_concurrent_cache.load(Ordering::Acquire);
        if limit > prev {
            state.apu_max_concurrent_cache.store(limit, Ordering::Release);
        }
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
            .or_insert_with(|| {
                let ms = ModelState::new(0, false, 128, 1);
                ms.baseline_tps.store(tps_x100, Ordering::Release);
                ms
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
            .or_insert_with(|| {
                let ms = ModelState::new(0, false, 128, 1);
                ms.baseline_p95_ms.store(p95_ms, Ordering::Release);
                ms
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

    // ── Phase 7: model state fields ─────────────────────────────────────

    fn is_preloading(&self, provider_id: Uuid, model: &str) -> bool {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.is_preloading.load(Ordering::Acquire)))
            .unwrap_or(false)
    }

    fn set_preloading(&self, provider_id: Uuid, model: &str, value: bool) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.is_preloading.store(value, Ordering::Release);
        }
    }

    fn preload_fail_count(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.preload_fail_count.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn record_preload_failure(&self, provider_id: Uuid, model: &str) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            let count = ms.preload_fail_count.fetch_add(1, Ordering::AcqRel) + 1;
            if count >= 3 {
                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                ms.preload_failed_at.store(now_ms, Ordering::Release);
                ms.preload_fail_count.store(0, Ordering::Release); // reset for next cycle
            }
        }
    }

    fn record_preload_success(&self, provider_id: Uuid, model: &str) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.preload_fail_count.store(0, Ordering::Release);
            ms.preload_failed_at.store(0, Ordering::Release);
        }
    }

    fn preload_failed_at(&self, provider_id: Uuid, model: &str) -> u64 {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.preload_failed_at.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn is_preload_excluded(&self, provider_id: Uuid, model: &str) -> bool {
        let failed_at = self.preload_failed_at(provider_id, model);
        if failed_at == 0 { return false; }
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        now_ms.saturating_sub(failed_at) < 300_000
    }

    fn is_pulling(&self, provider_id: Uuid, model: &str) -> bool {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.is_pulling.load(Ordering::Acquire)))
            .unwrap_or(false)
    }

    fn set_pulling(&self, provider_id: Uuid, model: &str, value: bool) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.is_pulling.store(value, Ordering::Release);
        }
    }

    fn is_dispatch_blocked(&self, provider_id: Uuid, model: &str) -> bool {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.dispatch_blocked.load(Ordering::Acquire)))
            .unwrap_or(false)
    }

    fn set_dispatch_blocked(&self, provider_id: Uuid, model: &str, value: bool) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.dispatch_blocked.store(value, Ordering::Release);
        }
    }

    fn pre_hard_max_concurrent(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.pre_hard_max_concurrent.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn set_pre_hard_max_concurrent(&self, provider_id: Uuid, model: &str, value: u32) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.pre_hard_max_concurrent.store(value, Ordering::Release);
        }
    }

    fn idle_since_secs(&self, provider_id: Uuid, model: &str) -> u64 {
        let last = self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.last_active_at.load(Ordering::Acquire)))
            .unwrap_or(0);
        if last == 0 { return u64::MAX; } // never active
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        now_ms.saturating_sub(last) / 1000
    }

    fn set_standby(&self, provider_id: Uuid, value: bool) {
        let state = self.get_or_create(provider_id);
        state.is_standby.store(value, Ordering::Release);
    }

    fn is_standby(&self, provider_id: Uuid) -> bool {
        self.providers.get(&provider_id)
            .map(|s| s.is_standby.load(Ordering::Acquire))
            .unwrap_or(false)
    }

    fn set_transition_until(&self, provider_id: Uuid, until_ms: u64) {
        let state = self.get_or_create(provider_id);
        state.transition_until.store(until_ms, Ordering::Release);
    }

    fn in_transition(&self, provider_id: Uuid) -> bool {
        self.providers.get(&provider_id)
            .map(|s| {
                let until = s.transition_until.load(Ordering::Acquire);
                if until == 0 { return false; }
                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                now_ms < until
            })
            .unwrap_or(false)
    }

    fn governor_cap(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers.get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.governor_cap.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn set_governor_cap(&self, provider_id: Uuid, model: &str, cap: u32) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.governor_cap.store(cap, Ordering::Release);
        }
    }

    fn sum_loaded_max_concurrent(&self, provider_id: Uuid) -> u32 {
        self.providers.get(&provider_id)
            .map(|s| {
                s.models.iter()
                    .filter(|e| e.is_loaded)
                    .map(|e| e.max_concurrent.load(Ordering::Acquire))
                    .sum()
            })
            .unwrap_or(0)
    }

    fn model_weight_mb(&self, provider_id: Uuid, model: &str) -> u64 {
        self.providers
            .get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.weight_mb))
            .unwrap_or(0)
    }

    fn stable_cycle_count(&self, provider_id: Uuid, model: &str) -> u32 {
        self.providers
            .get(&provider_id)
            .and_then(|s| s.models.get(model).map(|ms| ms.stable_cycle_count.load(Ordering::Acquire)))
            .unwrap_or(0)
    }

    fn increment_stable_cycle_count(&self, provider_id: Uuid, model: &str) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.stable_cycle_count.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn reset_stable_cycle_count(&self, provider_id: Uuid, model: &str) {
        let state = self.get_or_create(provider_id);
        if let Some(ms) = state.models.get(model) {
            ms.stable_cycle_count.store(0, Ordering::Release);
        }
    }

    fn last_mem_available_mb(&self, provider_id: Uuid) -> u32 {
        self.providers
            .get(&provider_id)
            .map(|s| s.last_mem_available_mb.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    fn set_last_mem_available_mb(&self, provider_id: Uuid, mb: u32) {
        self.get_or_create(provider_id).last_mem_available_mb.store(mb, Ordering::Release);
    }

    fn safety_permil(&self, provider_id: Uuid) -> u32 {
        self.providers
            .get(&provider_id)
            .map(|s| s.safety_permil.load(Ordering::Acquire))
            .unwrap_or(DEFAULT_SAFETY_PERMIL)
    }

    fn set_safety_permil(&self, provider_id: Uuid, permil: u32) {
        self.get_or_create(provider_id)
            .safety_permil
            .store(permil, Ordering::Release);
    }

    fn decay_safety_permil(&self, provider_id: Uuid) {
        let state = self.get_or_create(provider_id);
        let cur = state.safety_permil.load(Ordering::Acquire);
        if cur > DEFAULT_SAFETY_PERMIL {
            let new = cur.saturating_sub(SAFETY_DECAY_PERMIL).max(DEFAULT_SAFETY_PERMIL);
            state.safety_permil.store(new, Ordering::Release);
        }
    }

}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── should_block tests ───────────────────────────────────────────────

    fn make_model_state(max_concurrent: u32, active: u32) -> ModelState {
        let ms = ModelState::new(0, false, 0, max_concurrent);
        ms.active_count.store(active, Ordering::Release);
        ms
    }

    #[test]
    fn unlimited_never_blocks() {
        let ms = make_model_state(0, 1000);
        assert!(!VramPool::should_block(&ms, 0, 3));
        assert!(!VramPool::should_block(&ms, 5, 3));
        assert!(!VramPool::should_block(&ms, -5, 3));
    }

    #[test]
    fn no_probe_blocks_at_limit() {
        let ms = make_model_state(4, 3);
        assert!(!VramPool::should_block(&ms, 0, 3)); // below limit
        let ms = make_model_state(4, 4);
        assert!(VramPool::should_block(&ms, 0, 3)); // at limit
        let ms = make_model_state(4, 5);
        assert!(VramPool::should_block(&ms, 0, 3)); // above limit
    }

    #[test]
    fn probe_up_hard_cap() {
        // probe_permits=2, limit=4 → hard cap at 6
        let ms = make_model_state(4, 6);
        assert!(VramPool::should_block(&ms, 2, 3)); // at hard cap
    }

    #[test]
    fn probe_up_allows_every_nth() {
        // limit=4, active=4 (in probe zone), probe_rate=3
        // Should allow every 3rd attempt
        let ms = make_model_state(4, 4);
        let results: Vec<bool> = (0..6)
            .map(|_| VramPool::should_block(&ms, 2, 3))
            .collect();
        // probe_counter starts at 0: 0%3==0 → allow, 1%3!=0 → block, 2%3!=0 → block, 3%3==0 → allow...
        assert_eq!(results, vec![false, true, true, false, true, true]);
    }

    #[test]
    fn probe_down_blocks_every_nth() {
        // limit=4, probe_permits=-1 → effective=3, active=3 (in probe zone), probe_rate=3
        let ms = make_model_state(4, 3);
        let results: Vec<bool> = (0..6)
            .map(|_| VramPool::should_block(&ms, -1, 3))
            .collect();
        // 0%3==0 → block, 1%3!=0 → pass, 2%3!=0 → pass, 3%3==0 → block...
        assert_eq!(results, vec![true, false, false, true, false, false]);
    }

    #[test]
    fn probe_down_hard_limit_still_enforced() {
        let ms = make_model_state(4, 4);
        assert!(VramPool::should_block(&ms, -1, 3)); // at original limit
    }

    #[test]
    fn probe_down_minimum_effective_is_1() {
        // limit=2, probe_permits=-5 → effective = max(1, 2-5) = 1
        let ms = make_model_state(2, 0);
        assert!(!VramPool::should_block(&ms, -5, 3)); // active < effective
    }

    #[test]
    fn probe_rate_zero_with_probe_up_blocks() {
        let ms = make_model_state(4, 4);
        assert!(VramPool::should_block(&ms, 2, 0)); // probe_rate=0 → always block in zone
    }

    // ── compute_available tests ──────────────────────────────────────────

    fn make_provider_state(total: u64, kv: u64, safety: u32) -> ProviderVramState {
        ProviderVramState {
            total_mb: AtomicU64::new(total),
            reserved_kv_mb: Arc::new(AtomicU64::new(kv)),
            safety_permil: AtomicU32::new(safety),
            models: DashMap::new(),
            cached_loaded_weight_mb: AtomicU64::new(0),
            is_standby: AtomicBool::new(false),
            transition_until: AtomicU64::new(0),
            last_mem_available_mb: AtomicU32::new(0),
            total_active_count: Arc::new(AtomicU32::new(0)),
        }
    }

    #[test]
    fn zero_total_returns_max() {
        let state = make_provider_state(0, 0, 100);
        assert_eq!(VramPool::compute_available(&state), i64::MAX);
    }

    #[test]
    fn available_deducts_all_components() {
        // total=10000, loaded=3000, kv=500, buffer=512, safety=10% of 10000=1000
        let state = make_provider_state(10000, 500, 100);
        let ms = ModelState::new(3000, true, 128, 4);
        state.cached_loaded_weight_mb.store(3000, Ordering::Release); // keep cache consistent
        state.models.insert("model_a".to_string(), ms);
        let available = VramPool::compute_available(&state);
        // 10000 - 3000 - 500 - 512 - 1000 = 4988
        assert_eq!(available, 4988);
    }

    #[test]
    fn unloaded_models_not_counted_in_weight() {
        let state = make_provider_state(10000, 0, 100);
        let ms = ModelState::new(5000, false, 128, 4); // not loaded
        state.models.insert("model_a".to_string(), ms);
        let available = VramPool::compute_available(&state);
        // 10000 - 0(unloaded) - 0(kv) - 512 - 1000 = 8488
        assert_eq!(available, 8488);
    }

    #[test]
    fn safety_permil_200_is_20_percent() {
        let state = make_provider_state(10000, 0, 200);
        let available = VramPool::compute_available(&state);
        // 10000 - 0 - 0 - 512 - 2000(20%) = 7488
        assert_eq!(available, 7488);
    }

    #[test]
    fn available_can_go_negative() {
        // Overcommitted: more loaded than total
        let state = make_provider_state(1000, 0, 100);
        let ms = ModelState::new(2000, true, 128, 4);
        state.cached_loaded_weight_mb.store(2000, Ordering::Release); // keep cache consistent
        state.models.insert("big_model".to_string(), ms);
        let available = VramPool::compute_available(&state);
        assert!(available < 0, "available={available} should be negative");
    }

    // ── VramPool integration tests ───────────────────────────────────────

    #[test]
    fn try_reserve_zero_total_always_allows() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        // total_vram=0 (not probed) → always allow
        let permit = pool.try_reserve(pid, "test_model");
        assert!(permit.is_some());
    }

    #[test]
    fn try_reserve_respects_concurrency_limit() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_probe_config(0, 0); // no probing
        pool.set_max_concurrent(pid, "model", 2);

        let p1 = pool.try_reserve(pid, "model");
        let p2 = pool.try_reserve(pid, "model");
        let p3 = pool.try_reserve(pid, "model");

        assert!(p1.is_some());
        assert!(p2.is_some());
        assert!(p3.is_none(), "should block at max_concurrent=2");

        // Drop one permit → next should succeed
        drop(p1);
        let p4 = pool.try_reserve(pid, "model");
        assert!(p4.is_some());
    }

    #[test]
    fn try_reserve_vram_insufficient() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_total_vram(pid, 2000); // 2GB total
        pool.mark_model_loaded(pid, "big_model", 1800); // 1.8GB loaded
        pool.set_max_concurrent(pid, "big_model", 10);

        // KV needs at least 32MB, but available = 2000 - 1800 - 0 - 512 - 200 < 0
        let permit = pool.try_reserve(pid, "big_model");
        assert!(permit.is_none(), "should fail: insufficient VRAM");
    }

    #[test]
    fn try_reserve_apu_unified_memory_bypass() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_total_vram(pid, 1024); // DRM reports only 1GB
        pool.mark_model_loaded(pid, "qwen3:8b", 5000); // 5GB loaded (exceeds DRM total)
        pool.set_max_concurrent(pid, "qwen3:8b", 4);
        pool.set_probe_config(0, 0);

        // APU path: loaded_weight > total → trust concurrency limit, not VRAM
        let permit = pool.try_reserve(pid, "qwen3:8b");
        assert!(permit.is_some(), "APU path should allow when loaded > total");
    }

    #[test]
    fn permit_drop_releases_kv_and_count() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_total_vram(pid, 50000);
        pool.mark_model_loaded(pid, "model", 5000);
        pool.set_max_concurrent(pid, "model", 10);
        pool.set_probe_config(0, 0);

        let p1 = pool.try_reserve(pid, "model").unwrap();
        assert_eq!(pool.active_requests(pid, "model"), 1);
        let used_before = pool.used_vram_mb(pid);

        drop(p1);
        assert_eq!(pool.active_requests(pid, "model"), 0);
        let used_after = pool.used_vram_mb(pid);
        assert!(used_after < used_before, "KV should be released on drop");
    }

    #[test]
    fn safety_permil_bumps_on_oom() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_total_vram(pid, 1000);
        pool.mark_model_loaded(pid, "model", 800);
        pool.set_max_concurrent(pid, "model", 10);

        // This should fail (not enough VRAM) and bump safety_permil
        let _ = pool.try_reserve(pid, "model");

        let state = pool.providers.get(&pid).unwrap();
        let safety = state.safety_permil.load(Ordering::Acquire);
        assert!(safety > DEFAULT_SAFETY_PERMIL, "safety_permil should increase on OOM: {safety}");
    }

    #[test]
    fn mark_model_unloaded_resets_phase7_fields() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "model", 5000);

        // Set some phase 7 fields
        pool.set_preloading(pid, "model", true);
        {
            let state = pool.providers.get(&pid).unwrap();
            let ms = state.models.get("model").unwrap();
            ms.sample_count.store(10, Ordering::Release);
        }

        pool.mark_model_unloaded(pid, "model");

        // Verify resets
        assert!(!pool.is_preloading(pid, "model"));
        let state = pool.providers.get(&pid).unwrap();
        let ms = state.models.get("model").unwrap();
        assert_eq!(ms.sample_count.load(Ordering::Acquire), 0);
        assert!(ms.learning_epoch_started_at.load(Ordering::Acquire) > 0);
    }

    #[test]
    fn preload_failure_exclusion_after_3() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "model", 1000);

        pool.record_preload_failure(pid, "model");
        assert!(!pool.is_preload_excluded(pid, "model"));
        pool.record_preload_failure(pid, "model");
        assert!(!pool.is_preload_excluded(pid, "model"));
        pool.record_preload_failure(pid, "model"); // 3rd → exclusion
        assert!(pool.is_preload_excluded(pid, "model"));
    }

    #[test]
    fn preload_success_clears_exclusion() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "model", 1000);

        // Trigger exclusion
        for _ in 0..3 {
            pool.record_preload_failure(pid, "model");
        }
        assert!(pool.is_preload_excluded(pid, "model"));

        pool.record_preload_success(pid, "model");
        assert!(!pool.is_preload_excluded(pid, "model"));
    }

    // ── Standby / Transition tests ────────────────────────────────────────

    #[test]
    fn standby_defaults_false() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        assert!(!pool.is_standby(pid));
    }

    #[test]
    fn set_standby_toggles() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_standby(pid, true);
        assert!(pool.is_standby(pid));
        pool.set_standby(pid, false);
        assert!(!pool.is_standby(pid));
    }

    #[test]
    fn in_transition_defaults_false() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        assert!(!pool.in_transition(pid));
    }

    #[test]
    fn in_transition_with_future_deadline() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        // Use u64::MAX as deadline — deterministic, always in the future.
        pool.set_transition_until(pid, u64::MAX);
        assert!(pool.in_transition(pid));
    }

    #[test]
    fn in_transition_with_past_deadline() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_transition_until(pid, 1); // epoch+1ms — always in the past
        assert!(!pool.in_transition(pid));
    }

    // ── Governor cap tests ─────────────────────────────────────────────────

    #[test]
    fn governor_cap_defaults_zero() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        assert_eq!(pool.governor_cap(pid, "model"), 0);
    }

    #[test]
    fn set_governor_cap_persists() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "model", 1000);
        pool.set_governor_cap(pid, "model", 3);
        assert_eq!(pool.governor_cap(pid, "model"), 3);
    }

    #[test]
    fn governor_cap_limits_dispatch() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_probe_config(0, 0);
        pool.set_max_concurrent(pid, "model", 8);
        pool.mark_model_loaded(pid, "model", 1000);
        pool.set_governor_cap(pid, "model", 2);

        let p1 = pool.try_reserve(pid, "model");
        let p2 = pool.try_reserve(pid, "model");
        let p3 = pool.try_reserve(pid, "model");

        assert!(p1.is_some());
        assert!(p2.is_some());
        assert!(p3.is_none(), "governor_cap=2 should block 3rd request");
        drop(p1);
        drop(p2);
    }

    #[test]
    fn dispatch_blocked_blocks_all() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_probe_config(0, 0);
        pool.set_max_concurrent(pid, "model", 8);
        pool.set_dispatch_blocked(pid, "model", true);

        let p1 = pool.try_reserve(pid, "model");
        assert!(p1.is_none(), "dispatch_blocked should block all requests");
    }

    // ── sum_loaded_max_concurrent tests ─────────────────────────────────────

    #[test]
    fn sum_loaded_max_concurrent_empty() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        assert_eq!(pool.sum_loaded_max_concurrent(pid), 0);
    }

    #[test]
    fn sum_loaded_max_concurrent_counts_loaded_only() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "a", 1000);
        pool.set_max_concurrent(pid, "a", 4);
        pool.mark_model_loaded(pid, "b", 2000);
        pool.set_max_concurrent(pid, "b", 3);
        // "c" not loaded
        pool.set_max_concurrent(pid, "c", 5);

        assert_eq!(pool.sum_loaded_max_concurrent(pid), 7); // 4 + 3, not 12
    }

    // ── Property-based tests ─────────────────────────────────────────────

    proptest! {
        /// compute_available is always i64::MAX when total=0.
        #[test]
        fn zero_total_always_unlimited(kv in 0u32..10000, safety in 0u32..500) {
            let state = make_provider_state(0, kv, safety);
            prop_assert_eq!(VramPool::compute_available(&state), i64::MAX);
        }

        /// Higher safety_permil → lower available (monotonically decreasing).
        #[test]
        fn higher_safety_less_available(
            total in 1000u32..100000,
            kv in 0u32..1000,
            safety_a in 0u32..500,
            safety_b in 0u32..500,
        ) {
            let state_a = make_provider_state(total, kv, safety_a);
            let state_b = make_provider_state(total, kv, safety_b);
            let avail_a = VramPool::compute_available(&state_a);
            let avail_b = VramPool::compute_available(&state_b);
            if safety_a <= safety_b {
                prop_assert!(avail_a >= avail_b, "safety {safety_a}→{avail_a} < safety {safety_b}→{avail_b}");
            }
        }

        /// should_block with limit=0 (unlimited) never blocks.
        #[test]
        fn unlimited_never_blocks_prop(active in 0u32..1000, permits in -10i32..10, rate in 0u32..10) {
            let ms = make_model_state(0, active);
            prop_assert!(!VramPool::should_block(&ms, permits, rate));
        }

        /// should_block without probing: blocked iff active >= limit.
        #[test]
        fn no_probe_strict_limit(limit in 1u32..100, active in 0u32..200) {
            let ms = make_model_state(limit, active);
            let blocked = VramPool::should_block(&ms, 0, 3);
            prop_assert_eq!(blocked, active >= limit);
        }
    }

    // ── stable_cycle_count tests ─────────────────────────────────────────────

    #[test]
    fn stable_cycle_count_increments_and_resets() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        // Initialise model entry.
        pool.set_max_concurrent(pid, "m", 4);

        assert_eq!(pool.stable_cycle_count(pid, "m"), 0);
        pool.increment_stable_cycle_count(pid, "m");
        assert_eq!(pool.stable_cycle_count(pid, "m"), 1);
        pool.increment_stable_cycle_count(pid, "m");
        assert_eq!(pool.stable_cycle_count(pid, "m"), 2);
        pool.increment_stable_cycle_count(pid, "m");
        assert_eq!(pool.stable_cycle_count(pid, "m"), 3);

        pool.reset_stable_cycle_count(pid, "m");
        assert_eq!(pool.stable_cycle_count(pid, "m"), 0);
    }

    #[test]
    fn stable_cycle_baseline_gate_requires_three_cycles() {
        // Simulate the AIMD stable-cycle loop manually:
        // baseline update should only happen when count reaches >= 3.
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.set_max_concurrent(pid, "m", 4);

        let update_would_fire = |p: &VramPool| {
            p.increment_stable_cycle_count(pid, "m");
            p.stable_cycle_count(pid, "m") >= 3
        };

        assert!(!update_would_fire(&pool), "1st stable cycle must NOT update baseline");
        assert!(!update_would_fire(&pool), "2nd stable cycle must NOT update baseline");
        assert!(update_would_fire(&pool),  "3rd stable cycle MUST update baseline");
        assert!(update_would_fire(&pool),  "4th+ stable cycle MUST still update baseline");

        pool.reset_stable_cycle_count(pid, "m");
        assert!(!update_would_fire(&pool), "after reset: 1st stable cycle must NOT update");
    }

    #[test]
    fn stable_cycle_count_unknown_model_returns_zero() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        assert_eq!(pool.stable_cycle_count(pid, "unknown"), 0);
        // increment / reset on unknown model must not panic.
        pool.increment_stable_cycle_count(pid, "unknown");
        pool.reset_stable_cycle_count(pid, "unknown");
    }

    // ── FIX 7: provider_active_requests O(1) atomic cache ─────────────────

    #[test]
    fn provider_active_requests_is_zero_initially() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        assert_eq!(pool.provider_active_requests(pid), 0);
    }

    #[test]
    fn provider_active_requests_tracks_permits() {
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "m", 0);
        pool.set_total_vram(pid, 0); // 0 = always allow

        let permit1 = pool.try_reserve(pid, "m").expect("permit1");
        assert_eq!(pool.provider_active_requests(pid), 1);

        let permit2 = pool.try_reserve(pid, "m").expect("permit2");
        assert_eq!(pool.provider_active_requests(pid), 2);

        drop(permit1);
        assert_eq!(pool.provider_active_requests(pid), 1);

        drop(permit2);
        assert_eq!(pool.provider_active_requests(pid), 0);
    }

    // ── FIX 8: is_model_loaded uses global DashSet ─────────────────────────

    #[test]
    fn is_model_loaded_returns_true_when_any_provider_has_it() {
        let pool = VramPool::new();
        let pid1 = Uuid::now_v7();
        let pid2 = Uuid::now_v7();

        assert!(!pool.is_model_loaded("llama3"));

        pool.mark_model_loaded(pid1, "llama3", 1000);
        assert!(pool.is_model_loaded("llama3"));

        pool.mark_model_unloaded(pid1, "llama3");
        assert!(!pool.is_model_loaded("llama3"), "removed when last provider unloads");

        pool.mark_model_loaded(pid1, "llama3", 1000);
        pool.mark_model_loaded(pid2, "llama3", 1000);
        pool.mark_model_unloaded(pid1, "llama3");
        assert!(pool.is_model_loaded("llama3"), "still loaded on pid2");

        pool.mark_model_unloaded(pid2, "llama3");
        assert!(!pool.is_model_loaded("llama3"), "removed when all providers unload");
    }
}
