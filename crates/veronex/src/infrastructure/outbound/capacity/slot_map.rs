use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::ConcurrencyPort;

/// Maps (backend_id, model_name) → (Semaphore, max_permits).
///
/// This is the primary concurrency control primitive — replaces the old
/// `busy_backends: HashSet<Uuid>` which allowed only 1 job per backend.
///
/// The capacity analyzer updates `max_permits` every 5 minutes based on
/// available VRAM and model architecture.  Existing in-flight permits are
/// safely held by the old `Arc<Semaphore>` (RAII); the new semaphore starts
/// fresh for new acquisitions.
#[derive(Clone)]
pub struct ConcurrencySlotMap {
    inner: Arc<DashMap<(Uuid, String), (Arc<Semaphore>, u32)>>,
}

impl ConcurrencySlotMap {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Update the maximum concurrency for a (backend, model) pair.
    ///
    /// Called by the capacity analyzer every 5 minutes.
    /// Replaces the semaphore atomically; in-flight permits on the old
    /// semaphore are returned when their tasks finish (RAII — safe).
    pub fn update_capacity(&self, backend_id: Uuid, model: &str, new_max: u32) {
        let new_max = new_max.clamp(1, 8);
        self.inner.insert(
            (backend_id, model.to_string()),
            (Arc::new(Semaphore::new(new_max as usize)), new_max),
        );
    }

    /// Attempt a non-blocking slot acquisition.
    ///
    /// Returns `Some(permit)` if a slot was available, `None` if all slots
    /// are currently occupied.  The permit is RAII — dropping it releases
    /// the slot automatically.
    pub fn try_acquire(&self, backend_id: Uuid, model: &str) -> Option<OwnedSemaphorePermit> {
        let key = (backend_id, model.to_string());
        let entry = self
            .inner
            .entry(key)
            .or_insert_with(|| (Arc::new(Semaphore::new(1)), 1));
        entry.0.clone().try_acquire_owned().ok()
    }

    /// Current available (free) slots for a (backend, model) pair.
    pub fn available_slots(&self, backend_id: Uuid, model: &str) -> u32 {
        self.inner
            .get(&(backend_id, model.to_string()))
            .map(|e| e.0.available_permits() as u32)
            .unwrap_or(1)
    }

    /// Maximum configured slots for a (backend, model) pair.
    pub fn max_slots(&self, backend_id: Uuid, model: &str) -> u32 {
        self.inner
            .get(&(backend_id, model.to_string()))
            .map(|e| e.1)
            .unwrap_or(1)
    }

    /// Currently active (in-flight) slots = max − available.
    pub fn active_slots(&self, backend_id: Uuid, model: &str) -> u32 {
        self.max_slots(backend_id, model)
            .saturating_sub(self.available_slots(backend_id, model))
    }
}

impl Default for ConcurrencySlotMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ConcurrencyPort for ConcurrencySlotMap {
    fn try_acquire(&self, provider_id: Uuid, model: &str) -> Option<OwnedSemaphorePermit> {
        self.try_acquire(provider_id, model)
    }

    fn active_slots(&self, provider_id: Uuid, model: &str) -> u32 {
        self.active_slots(provider_id, model)
    }
}
