use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::{ConcurrencyPort, SlotPermit};

/// Per-(provider, model) concurrency state.
///
/// `active` tracks in-flight jobs (incremented on acquire, decremented on drop).
/// `max` is the VRAM-derived capacity limit, updated by the capacity analyzer.
///
/// Key invariant: `update_capacity()` only writes to `max`, never replaces
/// the `SlotState` entry, so in-flight `SlotPermit`s always point at the
/// same `active` counter.
struct SlotState {
    active: Arc<AtomicU32>,
    max: AtomicU32,
}

/// Maps (backend_id, model_name) → SlotState.
///
/// This is the primary concurrency control primitive — replaces the old
/// Semaphore-based implementation that orphaned permits on `update_capacity()`.
#[derive(Clone)]
pub struct ConcurrencySlotMap {
    inner: Arc<DashMap<(Uuid, String), Arc<SlotState>>>,
}

impl ConcurrencySlotMap {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Update the maximum concurrency for a (provider, model) pair.
    ///
    /// Called by the capacity analyzer every 5 minutes.
    /// Only stores the new max — never replaces the entry — so in-flight
    /// permits remain valid and the active counter is preserved.
    pub fn update_capacity(&self, backend_id: Uuid, model: &str, new_max: u32) {
        let new_max = new_max.clamp(1, 8);
        let key = (backend_id, model.to_string());
        self.inner
            .entry(key)
            .and_modify(|state| {
                state.max.store(new_max, Ordering::Release);
            })
            .or_insert_with(|| {
                Arc::new(SlotState {
                    active: Arc::new(AtomicU32::new(0)),
                    max: AtomicU32::new(new_max),
                })
            });
    }

    /// Attempt a non-blocking slot acquisition.
    ///
    /// Uses a CAS loop on `active` to atomically increment if below `max`.
    /// Returns `Some(SlotPermit)` if a slot was available, `None` if all
    /// slots are currently occupied.
    pub fn try_acquire(&self, backend_id: Uuid, model: &str) -> Option<SlotPermit> {
        let key = (backend_id, model.to_string());
        let state = self
            .inner
            .entry(key)
            .or_insert_with(|| {
                Arc::new(SlotState {
                    active: Arc::new(AtomicU32::new(0)),
                    max: AtomicU32::new(1),
                })
            })
            .value()
            .clone();

        loop {
            let cur = state.active.load(Ordering::Acquire);
            let max = state.max.load(Ordering::Acquire);
            if cur >= max {
                return None;
            }
            if state
                .active
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(SlotPermit::new(state.active.clone()));
            }
            // CAS failed — another thread won the race; retry.
        }
    }

    /// Current available (free) slots for a (provider, model) pair.
    pub fn available_slots(&self, backend_id: Uuid, model: &str) -> u32 {
        self.inner
            .get(&(backend_id, model.to_string()))
            .map(|e| {
                let max = e.max.load(Ordering::Acquire);
                let active = e.active.load(Ordering::Acquire);
                max.saturating_sub(active)
            })
            .unwrap_or(1)
    }

    /// Maximum configured slots for a (provider, model) pair.
    pub fn max_slots(&self, backend_id: Uuid, model: &str) -> u32 {
        self.inner
            .get(&(backend_id, model.to_string()))
            .map(|e| e.max.load(Ordering::Acquire))
            .unwrap_or(1)
    }

    /// Currently active (in-flight) slots.
    pub fn active_slots(&self, backend_id: Uuid, model: &str) -> u32 {
        self.inner
            .get(&(backend_id, model.to_string()))
            .map(|e| e.active.load(Ordering::Acquire))
            .unwrap_or(0)
    }
}

impl Default for ConcurrencySlotMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ConcurrencyPort for ConcurrencySlotMap {
    fn try_acquire(&self, provider_id: Uuid, model: &str) -> Option<SlotPermit> {
        self.try_acquire(provider_id, model)
    }

    fn active_slots(&self, provider_id: Uuid, model: &str) -> u32 {
        self.active_slots(provider_id, model)
    }
}
