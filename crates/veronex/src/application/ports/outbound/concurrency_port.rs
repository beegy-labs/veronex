use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;

/// Port for per-(backend, model) concurrency slot management.
///
/// Abstracts `ConcurrencySlotMap` so the application use-case layer is
/// decoupled from the concrete semaphore implementation.
pub trait ConcurrencyPort: Send + Sync {
    /// Non-blocking slot acquisition.
    ///
    /// Returns `Some(permit)` if a slot is available, `None` if fully occupied.
    /// Dropping the permit auto-releases the slot (RAII).
    fn try_acquire(&self, provider_id: Uuid, model: &str) -> Option<OwnedSemaphorePermit>;

    /// Number of currently active (in-flight) slots for a (provider, model) pair.
    fn active_slots(&self, provider_id: Uuid, model: &str) -> u32;
}
