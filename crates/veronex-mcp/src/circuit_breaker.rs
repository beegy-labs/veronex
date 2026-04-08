//! `McpCircuitBreaker` — per-MCP-server state machine.
//!
//! Separate from `veronex`'s existing `CircuitBreakerMap` (which handles Ollama).
//!
//! States: Closed → Open (5 consecutive failures) → HalfOpen (60 s) → Closed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tracing::warn;
use uuid::Uuid;

// ── State ─────────────────────────────────────────────────────────────────────

const FAILURE_THRESHOLD: u32 = 5;
const HALF_OPEN_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
enum State {
    Closed { consecutive_failures: u32 },
    Open { since: Instant },
    HalfOpen,
}

#[derive(Debug)]
struct ServerState {
    state: State,
}

impl ServerState {
    fn new() -> Self {
        Self { state: State::Closed { consecutive_failures: 0 } }
    }

    fn record_success(&mut self) {
        self.state = State::Closed { consecutive_failures: 0 };
    }

    fn record_failure(&mut self) {
        match &mut self.state {
            State::Closed { consecutive_failures } => {
                *consecutive_failures += 1;
                if *consecutive_failures >= FAILURE_THRESHOLD {
                    warn!("McpCircuitBreaker: opening circuit after {FAILURE_THRESHOLD} failures");
                    self.state = State::Open { since: Instant::now() };
                }
            }
            State::Open { .. } => {} // still open
            State::HalfOpen => {
                // Probe failed — go back to Open
                self.state = State::Open { since: Instant::now() };
            }
        }
    }

    /// Called before attempting a call. Promotes Open→HalfOpen when timeout elapsed.
    fn check_and_maybe_promote(&mut self) -> bool {
        if let State::Open { since } = &self.state {
            if since.elapsed() >= HALF_OPEN_AFTER {
                self.state = State::HalfOpen;
                return false; // allow the probe
            }
            return true; // still open
        }
        false
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub struct McpCircuitBreaker {
    servers: Arc<DashMap<Uuid, ServerState>>,
}

impl McpCircuitBreaker {
    pub fn new() -> Self {
        Self { servers: Arc::new(DashMap::new()) }
    }

    /// Returns `true` if calls to this server should be blocked.
    pub fn is_open(&self, server_id: Uuid) -> bool {
        let mut entry = self
            .servers
            .entry(server_id)
            .or_insert_with(ServerState::new);
        entry.check_and_maybe_promote()
    }

    /// Record a successful call.
    pub fn record_success(&self, server_id: Uuid) {
        self.servers
            .entry(server_id)
            .or_insert_with(ServerState::new)
            .record_success();
    }

    /// Record a failed call (timeout, protocol error, `isError`).
    pub fn record_failure(&self, server_id: Uuid) {
        self.servers
            .entry(server_id)
            .or_insert_with(ServerState::new)
            .record_failure();
    }

    /// Convenience: record based on result success/failure.
    pub fn record(&self, server_id: Uuid, success: bool) {
        if success {
            self.record_success(server_id);
        } else {
            self.record_failure(server_id);
        }
    }
}

impl Default for McpCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> McpCircuitBreaker {
        McpCircuitBreaker::new()
    }

    // ── Closed state ─────────────────────────────────────────────────────────

    #[test]
    fn new_server_is_closed() {
        let cb = fresh();
        let id = Uuid::new_v4();
        assert!(!cb.is_open(id));
    }

    #[test]
    fn success_keeps_closed() {
        let cb = fresh();
        let id = Uuid::new_v4();
        for _ in 0..10 {
            cb.record_success(id);
        }
        assert!(!cb.is_open(id));
    }

    #[test]
    fn failures_below_threshold_stay_closed() {
        let cb = fresh();
        let id = Uuid::new_v4();
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            cb.record_failure(id);
        }
        assert!(!cb.is_open(id));
    }

    // ── Closed → Open ────────────────────────────────────────────────────────

    #[test]
    fn threshold_failures_open_circuit() {
        let cb = fresh();
        let id = Uuid::new_v4();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(id);
        }
        assert!(cb.is_open(id));
    }

    #[test]
    fn success_after_failures_resets_to_closed() {
        let cb = fresh();
        let id = Uuid::new_v4();
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            cb.record_failure(id);
        }
        cb.record_success(id);
        // Counter is reset — needs FAILURE_THRESHOLD more failures to open.
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            cb.record_failure(id);
        }
        assert!(!cb.is_open(id));
    }

    // ── Open → HalfOpen ──────────────────────────────────────────────────────

    #[test]
    fn open_transitions_to_halfopen_after_timeout() {
        let mut state = ServerState::new();
        // Force open
        for _ in 0..FAILURE_THRESHOLD {
            state.record_failure();
        }
        // Simulate elapsed time by setting `since` to a past instant.
        // We can't easily manipulate Instant, so instead we verify that
        // is_open() returns false immediately (HalfOpen allow probe).
        // Use check_and_maybe_promote directly after mocking Open state.
        state.state = State::Open { since: Instant::now() - HALF_OPEN_AFTER - Duration::from_millis(1) };
        // check_and_maybe_promote should promote to HalfOpen and return false (allow probe)
        assert!(!state.check_and_maybe_promote());
        // State is now HalfOpen
        assert!(matches!(state.state, State::HalfOpen));
    }

    #[test]
    fn halfopen_failure_reopens() {
        let mut state = ServerState::new();
        state.state = State::HalfOpen;
        state.record_failure();
        assert!(matches!(state.state, State::Open { .. }));
    }

    #[test]
    fn halfopen_success_closes() {
        let mut state = ServerState::new();
        state.state = State::HalfOpen;
        state.record_success();
        assert!(matches!(state.state, State::Closed { consecutive_failures: 0 }));
    }

    // ── record() convenience ─────────────────────────────────────────────────
    // (failure_threshold_is_reasonable removed: pure constant assertion)

    #[test]
    fn record_convenience_delegates_correctly() {
        let cb = fresh();
        let id = Uuid::new_v4();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record(id, false); // failure
        }
        assert!(cb.is_open(id));
        cb.record(id, true); // success — but state machine stays Open until HalfOpen probe
        // After success from Open state: state machine remains Open (not yet probed via is_open)
        // record_success() on an Open server resets to Closed immediately.
        assert!(!cb.is_open(id));
    }

    // ── Server isolation ─────────────────────────────────────────────────────

    #[test]
    fn servers_are_isolated() {
        let cb = fresh();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(a);
        }
        // Server A is open; server B is unaffected
        assert!(cb.is_open(a));
        assert!(!cb.is_open(b));
    }

    #[test]
    fn failure_in_halfopen_reopens_only_that_server() {
        let cb = fresh();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        {
            let mut entry = cb.servers.entry(a).or_insert_with(ServerState::new);
            entry.state = State::HalfOpen;
        }
        cb.record_failure(a);
        assert!(cb.is_open(a));
        assert!(!cb.is_open(b));
    }

}
// (new_state_starts_closed_with_zero_failures removed: trivial struct init covered by new_server_is_closed)
// (failure_below_threshold_stays_closed removed: duplicate of failures_below_threshold_stay_closed)
