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

    /// Seconds elapsed since Hard throttle was entered (None if not in Hard state).
    /// Used by placement_planner for 60s forced-drain and 90s watchdog (SDD §3).
    fn hard_since_elapsed_secs(&self, provider_id: Uuid) -> Option<u64>;

    /// Proactively transition a provider from Hard → Cooldown.
    /// Called by placement_planner when active_requests drops to 0 under Hard gate (SDD §3).
    fn set_cooldown(&self, provider_id: Uuid);
}
