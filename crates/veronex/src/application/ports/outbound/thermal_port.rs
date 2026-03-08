use uuid::Uuid;

use crate::domain::enums::ThrottleLevel;

/// Port for reading per-provider thermal throttle state.
///
/// Abstracts `ThermalThrottleMap` so the application use-case layer is
/// decoupled from the concrete thermal monitoring implementation.
pub trait ThermalPort: Send + Sync {
    /// Current throttle level for a provider (Normal if no state recorded).
    fn get_level(&self, provider_id: Uuid) -> ThrottleLevel;
}
