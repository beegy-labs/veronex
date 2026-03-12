use uuid::Uuid;

use crate::domain::enums::ThrottleLevel;

/// Port for reading per-provider thermal throttle state.
///
/// Abstracts `ThermalThrottleMap` so the application use-case layer is
/// decoupled from the concrete thermal monitoring implementation.
pub trait ThermalPort: Send + Sync {
    /// Current throttle level for a provider (Normal if no state recorded).
    fn get_level(&self, provider_id: Uuid) -> ThrottleLevel;

    /// Temperature-based performance scaling factor (0.0–1.0).
    /// Used by queue scoring to reduce age_bonus on hot servers.
    fn perf_factor(&self, provider_id: Uuid) -> f32;

    /// Global perf_factor: minimum across all tracked providers.
    /// Conservative estimate for queue window scoring.
    fn global_perf_factor(&self) -> f32;
}
