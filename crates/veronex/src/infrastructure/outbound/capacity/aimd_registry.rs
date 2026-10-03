//! Phase 4 — AIMD per-provider window state.
//!
//! Tracks the current concurrent-request window `W` for every managed
//! provider. The router admission gate (`admission.rs`) consults `W`
//! before dispatching; the analyzer loop (`analyzer.rs`) updates it
//! every `control_interval` based on the dual-pressure signal.
//!
//! Lifecycle hooks:
//! - `register(provider_id, n_parallel)` — called by ProcessManager
//!   *after* warmup passes (Phase 3). `W` starts at 1 per Concur slow-start.
//! - `unregister(provider_id)` — called when the process stops.

use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AimdState {
    /// Current window — number of concurrent requests we'll admit.
    pub window: u32,
    /// Hard upper bound from the Modelfile runtime (`n_parallel` flag
    /// passed to `llama-server`). Window will never exceed this.
    pub n_parallel: u32,
    /// When `window` was last updated. Allows the controller to enforce
    /// a minimum interval between adjustments.
    pub last_update: Instant,
}

impl AimdState {
    /// Per Concur (arXiv 2601.22705) slow start: every newly-warmed-up
    /// provider begins at `W = 1` and grows additively.
    pub fn fresh(n_parallel: u32) -> Self {
        Self {
            window: 1,
            n_parallel: n_parallel.max(1),
            last_update: Instant::now(),
        }
    }
}

/// Cheap to clone (`Arc`-of-state). One handle is shared between the
/// analyzer loop, admission gate, and admin endpoints.
#[derive(Clone, Default)]
pub struct AimdRegistry {
    tracked: Arc<DashMap<Uuid, AimdState>>,
}

impl AimdRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a provider with a fresh state. Idempotent — re-registering
    /// is a no-op (an already-tracked provider keeps its current
    /// window so a transient analyzer restart doesn't reset learning).
    pub fn register(&self, provider_id: Uuid, n_parallel: u32) {
        self.tracked
            .entry(provider_id)
            .or_insert_with(|| AimdState::fresh(n_parallel));
    }

    /// Drop a provider — its process was stopped or removed.
    pub fn unregister(&self, provider_id: Uuid) {
        self.tracked.remove(&provider_id);
    }

    /// Look up the current window. Falls back to `n_parallel` (assumed
    /// safe) when the provider isn't tracked — admission shouldn't
    /// block a brand-new provider on a registry race.
    pub fn get_window(&self, provider_id: Uuid) -> Option<u32> {
        self.tracked.get(&provider_id).map(|s| s.window)
    }

    /// Atomically replace the window for a provider. Returns whether
    /// the entry existed.
    pub fn set_window(&self, provider_id: Uuid, new_window: u32) -> bool {
        if let Some(mut entry) = self.tracked.get_mut(&provider_id) {
            entry.window = new_window.max(1).min(entry.n_parallel);
            entry.last_update = Instant::now();
            true
        } else {
            false
        }
    }

    /// Snapshot for the analyzer loop. The clone is cheap (3 fields).
    pub fn tracked_providers(&self) -> Vec<(Uuid, AimdState)> {
        self.tracked
            .iter()
            .map(|e| (*e.key(), e.value().clone()))
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn fresh_starts_at_one_per_concur_slow_start() {
        let s = AimdState::fresh(8);
        assert_eq!(s.window, 1);
        assert_eq!(s.n_parallel, 8);
    }

    #[test]
    fn n_parallel_floor_one() {
        assert_eq!(AimdState::fresh(0).n_parallel, 1);
    }

    #[test]
    fn register_is_idempotent() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.set_window(id, 3);
        reg.register(id, 8); // ignored — keeps window=3, n_parallel=4
        let snap = reg.tracked_providers();
        let s = snap.iter().find(|(p, _)| *p == id).map(|(_, st)| st).unwrap();
        assert_eq!(s.window, 3);
        assert_eq!(s.n_parallel, 4);
    }

    #[test]
    fn set_window_clamps_to_bounds() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.set_window(id, 100);
        assert_eq!(reg.get_window(id), Some(4)); // clamped to n_parallel
        reg.set_window(id, 0);
        assert_eq!(reg.get_window(id), Some(1)); // clamped up to 1
    }

    #[test]
    fn unregister_removes_entry() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.unregister(id);
        assert_eq!(reg.get_window(id), None);
    }
}
