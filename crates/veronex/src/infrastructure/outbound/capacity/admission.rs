//! Phase 4 — router-side AIMD admission gate.
//!
//! The window `W` only matters if something gates dispatch on it.
//! [`AimdAdmission::acquire`] increments an in-flight counter, compares
//! against `registry.get_window`, and either:
//!
//! - returns a [`AdmissionGuard`] (decrements on drop), or
//! - rolls back the increment and returns `None` so the router can
//!   fall through to another provider (or surface 503).
//!
//! This is the *only* place AIMD's window has effect. Without it the
//! analyzer would write `W` and nothing would consult it.

use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

use crate::infrastructure::outbound::capacity::aimd_registry::AimdRegistry;

/// Cheap to clone (`Arc`-of-state).
#[derive(Clone)]
pub struct AimdAdmission {
    registry: AimdRegistry,
    inflight: Arc<DashMap<Uuid, u32>>,
}

impl AimdAdmission {
    pub fn new(registry: AimdRegistry) -> Self {
        Self {
            registry,
            inflight: Arc::new(DashMap::new()),
        }
    }

    /// Try to admit one request to `provider_id`. Returns the guard on
    /// success; `None` when the window is full so the router can pick
    /// a different provider.
    ///
    /// Untracked providers (not in the AIMD registry) are admitted
    /// without a window check — the router admits them at full
    /// `n_parallel` until the analyzer registers them.
    pub fn acquire(&self, provider_id: Uuid) -> Option<AdmissionGuard> {
        let window = match self.registry.get_window(provider_id) {
            Some(w) => w,
            None => {
                // Provider not yet tracked — admit without gating.
                let mut entry = self.inflight.entry(provider_id).or_insert(0);
                *entry = entry.saturating_add(1);
                return Some(AdmissionGuard {
                    inflight: self.inflight.clone(),
                    provider_id,
                });
            }
        };

        // CAS-style increment-then-check using DashMap's per-bucket
        // lock. We optimistically bump and roll back when over the
        // window — keeps the hot path lock-free in the success case.
        let mut entry = self.inflight.entry(provider_id).or_insert(0);
        if *entry >= window {
            return None;
        }
        *entry = entry.saturating_add(1);
        Some(AdmissionGuard {
            inflight: self.inflight.clone(),
            provider_id,
        })
    }

    /// Current in-flight count. Surfaced by the admin "AIMD live state"
    /// endpoint and consumed by the analyzer for KV utilization decay.
    pub fn inflight(&self, provider_id: Uuid) -> u32 {
        self.inflight.get(&provider_id).map(|e| *e).unwrap_or(0)
    }

    /// Snapshot for admin UIs.
    pub fn snapshot(&self) -> Vec<(Uuid, u32)> {
        self.inflight
            .iter()
            .map(|e| (*e.key(), *e.value()))
            .collect()
    }
}

/// RAII guard. Drop decrements `inflight`. Hold one for the lifetime
/// of the inference call (router-level pattern).
pub struct AdmissionGuard {
    inflight: Arc<DashMap<Uuid, u32>>,
    provider_id: Uuid,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        if let Some(mut entry) = self.inflight.get_mut(&self.provider_id) {
            *entry = entry.saturating_sub(1);
        }
    }
}

impl std::fmt::Debug for AdmissionGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionGuard")
            .field("provider_id", &self.provider_id)
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn acquire_below_window_succeeds() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.set_window(id, 2);
        let adm = AimdAdmission::new(reg);
        let _g1 = adm.acquire(id).unwrap();
        let _g2 = adm.acquire(id).unwrap();
        assert_eq!(adm.inflight(id), 2);
    }

    #[test]
    fn acquire_at_window_returns_none() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.set_window(id, 1);
        let adm = AimdAdmission::new(reg);
        let _g1 = adm.acquire(id).unwrap();
        assert!(adm.acquire(id).is_none());
        assert_eq!(adm.inflight(id), 1); // failed acquire didn't bump count
    }

    #[test]
    fn drop_decrements_inflight() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.set_window(id, 4);
        let adm = AimdAdmission::new(reg);
        {
            let _g = adm.acquire(id).unwrap();
            assert_eq!(adm.inflight(id), 1);
        }
        assert_eq!(adm.inflight(id), 0);
    }

    #[test]
    fn untracked_provider_admitted_without_gate() {
        let adm = AimdAdmission::new(AimdRegistry::new());
        let id = Uuid::now_v7();
        // Not in the registry — should still admit.
        let _g = adm.acquire(id).unwrap();
        assert_eq!(adm.inflight(id), 1);
    }

    #[test]
    fn shrinking_window_below_inflight_blocks_new_acquires() {
        let reg = AimdRegistry::new();
        let id = Uuid::now_v7();
        reg.register(id, 4);
        reg.set_window(id, 4);
        let adm = AimdAdmission::new(reg.clone());
        let _g1 = adm.acquire(id).unwrap();
        let _g2 = adm.acquire(id).unwrap();
        // Analyzer cuts the window mid-flight.
        reg.set_window(id, 1);
        // New acquire blocks because inflight (2) >= window (1).
        assert!(adm.acquire(id).is_none());
    }
}
