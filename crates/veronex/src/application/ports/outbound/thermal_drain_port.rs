use uuid::Uuid;

/// Port for force-cancelling in-flight jobs on a thermally-throttled provider.
///
/// Used by placement_planner to implement the SDD §3 Hard Gate 60s forced drain:
/// if a provider has been in Hard throttle for ≥60s and still has active requests,
/// all in-flight jobs assigned to it are cancelled so VramPermits drop and
/// active_count reaches 0, enabling the Cooldown transition.
pub trait ThermalDrainPort: Send + Sync {
    /// Cancel all in-flight jobs currently assigned to `provider_id`.
    /// Returns the number of cancel signals sent.
    fn cancel_jobs_for_provider(&self, provider_id: Uuid) -> usize;
}
