use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use uuid::Uuid;

/// RAII slot permit — decrements the active counter on drop.
///
/// Replaces `OwnedSemaphorePermit` so that `update_capacity()` can adjust
/// max slots without orphaning in-flight permits (the old Semaphore-swap bug).
pub struct SlotPermit {
    active: Arc<AtomicU32>,
}

impl SlotPermit {
    pub(crate) fn new(active: Arc<AtomicU32>) -> Self {
        Self { active }
    }
}

impl Drop for SlotPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::Release);
    }
}

/// Port for per-((provider, model)) concurrency slot management.
///
/// Abstracts `ConcurrencySlotMap` so the application use-case layer is
/// decoupled from the concrete atomic implementation.
pub trait ConcurrencyPort: Send + Sync {
    /// Non-blocking slot acquisition.
    ///
    /// Returns `Some(permit)` if a slot is available, `None` if fully occupied.
    /// Dropping the permit auto-releases the slot (RAII).
    fn try_acquire(&self, provider_id: Uuid, model: &str) -> Option<SlotPermit>;

    /// Number of currently active (in-flight) slots for a (provider, model) pair.
    fn active_slots(&self, provider_id: Uuid, model: &str) -> u32;
}
