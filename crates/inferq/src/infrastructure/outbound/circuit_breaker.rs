use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const FAILURE_THRESHOLD: u32 = 5;
const COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,
    Open { until: Instant },
    HalfOpen,
}

struct BackendCircuit {
    state: CircuitState,
    consecutive_failures: u32,
}

impl BackendCircuit {
    fn new() -> Self {
        Self { state: CircuitState::Closed, consecutive_failures: 0 }
    }
}

/// Thread-safe per-backend circuit breaker map.
///
/// Transitions:
/// - Closed → Open when N consecutive failures are recorded.
/// - Open → HalfOpen after COOLDOWN_SECS elapses.
/// - HalfOpen → Closed on success.
/// - HalfOpen → Open on failure (resets cooldown).
pub struct CircuitBreakerMap {
    inner: Mutex<HashMap<Uuid, BackendCircuit>>,
}

impl CircuitBreakerMap {
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }

    /// Returns true if requests are allowed for this backend.
    pub fn is_allowed(&self, backend_id: Uuid) -> bool {
        let mut map = self.inner.lock().expect("circuit breaker lock poisoned");
        let entry = map.entry(backend_id).or_insert_with(BackendCircuit::new);
        match &entry.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open { until } => {
                if Instant::now() >= *until {
                    entry.state = CircuitState::HalfOpen;
                    tracing::info!(
                        backend_id = %backend_id,
                        "circuit breaker half-open — probing backend"
                    );
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Call after a successful inference on this backend.
    pub fn on_success(&self, backend_id: Uuid) {
        let mut map = self.inner.lock().expect("circuit breaker lock poisoned");
        if let Some(entry) = map.get_mut(&backend_id) {
            if entry.state != CircuitState::Closed {
                tracing::info!(
                    backend_id = %backend_id,
                    "circuit breaker closed — backend recovered"
                );
            }
            entry.state = CircuitState::Closed;
            entry.consecutive_failures = 0;
        }
    }

    /// Call after a failed inference on this backend.
    pub fn on_failure(&self, backend_id: Uuid) {
        let mut map = self.inner.lock().expect("circuit breaker lock poisoned");
        let entry = map.entry(backend_id).or_insert_with(BackendCircuit::new);
        entry.consecutive_failures += 1;
        if entry.consecutive_failures >= FAILURE_THRESHOLD
            || entry.state == CircuitState::HalfOpen
        {
            entry.state = CircuitState::Open { until: Instant::now() + COOLDOWN };
            entry.consecutive_failures = 0;
            tracing::warn!(
                backend_id = %backend_id,
                "circuit breaker opened — backend isolated for {}s",
                COOLDOWN.as_secs()
            );
        }
    }

    /// Returns a snapshot of all open circuits for diagnostics.
    pub fn open_circuits(&self) -> Vec<Uuid> {
        let map = self.inner.lock().expect("circuit breaker lock poisoned");
        map.iter()
            .filter(|(_, c)| matches!(c.state, CircuitState::Open { .. }))
            .map(|(id, _)| *id)
            .collect()
    }
}

impl Default for CircuitBreakerMap {
    fn default() -> Self { Self::new() }
}
