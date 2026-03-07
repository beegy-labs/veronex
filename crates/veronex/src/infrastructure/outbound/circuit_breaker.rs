use std::time::Instant;
use dashmap::DashMap;
use uuid::Uuid;

use crate::application::ports::outbound::circuit_breaker_port::CircuitBreakerPort;
use crate::domain::constants::CIRCUIT_BREAKER_COOLDOWN;

/// Open circuit after N consecutive provider failures to prevent cascading.
const FAILURE_THRESHOLD: u32 = 5;

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,
    Open { until: Instant },
    HalfOpen,
}

struct ProviderCircuit {
    state: CircuitState,
    consecutive_failures: u32,
}

impl ProviderCircuit {
    fn new() -> Self {
        Self { state: CircuitState::Closed, consecutive_failures: 0 }
    }
}

/// Lock-free per-provider circuit breaker map backed by DashMap.
///
/// Transitions:
/// - Closed → Open when N consecutive failures are recorded.
/// - Open → HalfOpen after COOLDOWN elapses.
/// - HalfOpen → Closed on success.
/// - HalfOpen → Open on failure (resets cooldown).
pub struct CircuitBreakerMap {
    inner: DashMap<Uuid, ProviderCircuit>,
}

impl CircuitBreakerMap {
    pub fn new() -> Self {
        Self { inner: DashMap::new() }
    }

    /// Returns true if requests are allowed for this provider.
    pub fn is_allowed(&self, provider_id: Uuid) -> bool {
        let mut entry = self.inner.entry(provider_id).or_insert_with(ProviderCircuit::new);
        match &entry.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open { until } => {
                if Instant::now() >= *until {
                    entry.state = CircuitState::HalfOpen;
                    tracing::info!(
                        provider_id = %provider_id,
                        "circuit breaker half-open — probing provider"
                    );
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Call after a successful inference on this provider.
    pub fn on_success(&self, provider_id: Uuid) {
        if let Some(mut entry) = self.inner.get_mut(&provider_id) {
            if entry.state != CircuitState::Closed {
                tracing::info!(
                    provider_id = %provider_id,
                    "circuit breaker closed — provider recovered"
                );
            }
            entry.state = CircuitState::Closed;
            entry.consecutive_failures = 0;
        }
    }

    /// Call after a failed inference on this provider.
    pub fn on_failure(&self, provider_id: Uuid) {
        let mut entry = self.inner.entry(provider_id).or_insert_with(ProviderCircuit::new);
        entry.consecutive_failures += 1;
        if entry.consecutive_failures >= FAILURE_THRESHOLD
            || entry.state == CircuitState::HalfOpen
        {
            entry.state = CircuitState::Open { until: Instant::now() + CIRCUIT_BREAKER_COOLDOWN };
            entry.consecutive_failures = 0;
            tracing::warn!(
                provider_id = %provider_id,
                "circuit breaker opened — provider isolated for {}s",
                CIRCUIT_BREAKER_COOLDOWN.as_secs()
            );
        }
    }

    /// Returns a snapshot of all open circuits for diagnostics.
    pub fn open_circuits(&self) -> Vec<Uuid> {
        self.inner
            .iter()
            .filter(|r| matches!(r.state, CircuitState::Open { .. }))
            .map(|r| *r.key())
            .collect()
    }
}

impl Default for CircuitBreakerMap {
    fn default() -> Self { Self::new() }
}

impl CircuitBreakerPort for CircuitBreakerMap {
    fn is_allowed(&self, provider_id: Uuid) -> bool {
        self.is_allowed(provider_id)
    }

    fn on_success(&self, provider_id: Uuid) {
        self.on_success(provider_id)
    }

    fn on_failure(&self, provider_id: Uuid) {
        self.on_failure(provider_id)
    }
}
