use std::collections::VecDeque;
use std::time::Instant;
use dashmap::DashMap;
use uuid::Uuid;

use crate::application::ports::outbound::circuit_breaker_port::CircuitBreakerPort;
use crate::domain::constants::{
    CIRCUIT_BREAKER_COOLDOWN,
    CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES,
    CIRCUIT_BREAKER_LATENCY_WINDOW,
    CIRCUIT_BREAKER_P99_THRESHOLD_MS,
};

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
    /// Circular buffer of recent latencies (ms), capped at LATENCY_WINDOW.
    latencies: VecDeque<u64>,
}

impl ProviderCircuit {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            latencies: VecDeque::with_capacity(CIRCUIT_BREAKER_LATENCY_WINDOW),
        }
    }

    /// Push a latency sample, evicting the oldest if the window is full.
    fn push_latency(&mut self, latency_ms: u64) {
        if self.latencies.len() >= CIRCUIT_BREAKER_LATENCY_WINDOW {
            self.latencies.pop_front();
        }
        self.latencies.push_back(latency_ms);
    }

    /// Compute P99 latency from the current window. Returns `None` if fewer
    /// than `CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES` samples exist.
    fn p99_latency_ms(&self) -> Option<u64> {
        let n = self.latencies.len();
        if n < CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES {
            return None;
        }
        let mut sorted: Vec<u64> = self.latencies.iter().copied().collect();
        sorted.sort_unstable();
        // P99 index: ceil(0.99 * n) - 1, clamped to valid range.
        let idx = ((n as f64 * 0.99).ceil() as usize).saturating_sub(1).min(n - 1);
        Some(sorted[idx])
    }
}

/// Lock-free per-provider circuit breaker map backed by DashMap.
///
/// Transitions:
/// - Closed → Open when N consecutive failures are recorded.
/// - Open → HalfOpen after COOLDOWN elapses.
/// - HalfOpen → Closed on success.
/// - HalfOpen → Open on failure (resets cooldown).
/// - Closed → HalfOpen when P99 latency exceeds threshold (soft degradation).
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
            CircuitState::Closed | CircuitState::HalfOpen => true,
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

    /// Record a latency sample and soft-degrade to HalfOpen if P99 exceeds threshold.
    pub fn record_latency(&self, provider_id: Uuid, latency_ms: u64) {
        let mut entry = self.inner.entry(provider_id).or_insert_with(ProviderCircuit::new);
        entry.push_latency(latency_ms);

        // Only trigger soft degradation from Closed state — don't override Open.
        if entry.state != CircuitState::Closed {
            return;
        }

        if let Some(p99) = entry.p99_latency_ms()
            && p99 > CIRCUIT_BREAKER_P99_THRESHOLD_MS
        {
            entry.state = CircuitState::HalfOpen;
            tracing::warn!(
                provider_id = %provider_id,
                p99_ms = p99,
                threshold_ms = CIRCUIT_BREAKER_P99_THRESHOLD_MS,
                "circuit breaker half-open — P99 latency exceeded threshold"
            );
        }
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

    fn record_latency(&self, provider_id: Uuid, latency_ms: u64) {
        self.record_latency(provider_id, latency_ms)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn test_provider_id() -> Uuid {
        Uuid::nil()
    }

    #[test]
    fn latency_recording_stores_samples() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        for i in 0..10 {
            cb.record_latency(pid, i * 100);
        }

        let entry = cb.inner.get(&pid).unwrap();
        assert_eq!(entry.latencies.len(), 10);
        assert_eq!(entry.latencies[0], 0);
        assert_eq!(entry.latencies[9], 900);
    }

    #[test]
    fn latency_window_slides_at_capacity() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        // Fill the window completely
        for i in 0..CIRCUIT_BREAKER_LATENCY_WINDOW {
            cb.record_latency(pid, i as u64);
        }

        {
            let entry = cb.inner.get(&pid).unwrap();
            assert_eq!(entry.latencies.len(), CIRCUIT_BREAKER_LATENCY_WINDOW);
            assert_eq!(entry.latencies[0], 0);
        }

        // Push one more — oldest should be evicted
        cb.record_latency(pid, 9999);

        let entry = cb.inner.get(&pid).unwrap();
        assert_eq!(entry.latencies.len(), CIRCUIT_BREAKER_LATENCY_WINDOW);
        assert_eq!(entry.latencies[0], 1, "oldest sample (0) should be evicted");
        assert_eq!(
            *entry.latencies.back().unwrap(), 9999,
            "newest sample should be at the end"
        );
    }

    #[test]
    fn p99_returns_none_below_min_samples() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        for i in 0..(CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES - 1) {
            cb.record_latency(pid, i as u64 * 1000);
        }

        let entry = cb.inner.get(&pid).unwrap();
        assert!(entry.p99_latency_ms().is_none());
    }

    #[test]
    fn p99_calculation_correct() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        // Insert 100 samples: 0..99 ms. P99 of [0..99] = value at index 98 = 98.
        for i in 0..100u64 {
            cb.record_latency(pid, i);
        }

        let entry = cb.inner.get(&pid).unwrap();
        let p99 = entry.p99_latency_ms().unwrap();
        assert_eq!(p99, 98, "P99 of 0..99 should be 98");
    }

    #[test]
    fn p99_threshold_triggers_half_open() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        // Insert enough low-latency samples (below threshold)
        for _ in 0..(CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES - 1) {
            cb.record_latency(pid, 100);
        }

        // Circuit should still be Closed
        assert!(cb.is_allowed(pid));
        {
            let entry = cb.inner.get(&pid).unwrap();
            assert_eq!(entry.state, CircuitState::Closed);
        }

        // Push samples that exceed threshold — enough to push P99 over
        for _ in 0..CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES {
            cb.record_latency(pid, CIRCUIT_BREAKER_P99_THRESHOLD_MS + 1000);
        }

        // Circuit should now be HalfOpen (soft degradation)
        let entry = cb.inner.get(&pid).unwrap();
        assert_eq!(
            entry.state,
            CircuitState::HalfOpen,
            "P99 exceeded threshold — should be HalfOpen"
        );
    }

    #[test]
    fn p99_does_not_override_open_state() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        // Force circuit to Open via failures
        for _ in 0..FAILURE_THRESHOLD {
            cb.on_failure(pid);
        }

        {
            let entry = cb.inner.get(&pid).unwrap();
            assert!(matches!(entry.state, CircuitState::Open { .. }));
        }

        // Record high-latency samples — should NOT change state from Open
        for _ in 0..(CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES + 5) {
            cb.record_latency(pid, CIRCUIT_BREAKER_P99_THRESHOLD_MS + 5000);
        }

        let entry = cb.inner.get(&pid).unwrap();
        assert!(
            matches!(entry.state, CircuitState::Open { .. }),
            "Latency degradation must not override Open state"
        );
    }

    #[test]
    fn success_resets_to_closed_after_latency_half_open() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        // Trigger latency-based HalfOpen
        for _ in 0..(CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES + 5) {
            cb.record_latency(pid, CIRCUIT_BREAKER_P99_THRESHOLD_MS + 1000);
        }

        {
            let entry = cb.inner.get(&pid).unwrap();
            assert_eq!(entry.state, CircuitState::HalfOpen);
        }

        // Success should recover to Closed
        cb.on_success(pid);

        let entry = cb.inner.get(&pid).unwrap();
        assert_eq!(entry.state, CircuitState::Closed);
    }

    #[test]
    fn failure_based_logic_unchanged() {
        let cb = CircuitBreakerMap::new();
        let pid = test_provider_id();

        // Below threshold — still Closed
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            cb.on_failure(pid);
        }
        assert!(cb.is_allowed(pid));

        // Hit threshold — now Open
        cb.on_failure(pid);
        assert!(!cb.is_allowed(pid));

        let entry = cb.inner.get(&pid).unwrap();
        assert!(matches!(entry.state, CircuitState::Open { .. }));
    }
}
