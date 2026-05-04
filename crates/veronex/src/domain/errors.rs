use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    // ── Input validation ──────────────────────────────────────────────────
    #[error("validation error: {0}")]
    Validation(String),

    // ── Resource lookup ───────────────────────────────────────────────────
    #[error("not found: {0}")]
    NotFound(String),

    // ── Auth / access control ─────────────────────────────────────────────
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("expired API key: {0}")]
    ExpiredKey(String),

    #[error("invalid API key")]
    InvalidKey,

    #[error("forbidden: {0}")]
    Forbidden(String),

    // ── Rate limiting ─────────────────────────────────────────────────────
    #[error("rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    // ── Provider / inference ──────────────────────────────────────────────
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),

    #[error("inference failed: {0}")]
    InferenceFailed(String),

    #[error("queue full: {0}")]
    QueueFull(String),

    // ── Configuration ─────────────────────────────────────────────────────
    #[error("configuration error: {0}")]
    Configuration(String),

    // ── Crypto (encrypt / decrypt / key derivation) ──────────────────────
    #[error("crypto error: {0}")]
    Crypto(String),

    // ── Conflict ─────────────────────────────────────────────────────────
    #[error("conflict: {0}")]
    Conflict(String),

    // ── Internal (infrastructure-originated) ────────────────────────────
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

// ── Lifecycle (model load/unload) ───────────────────────────────────────────
//
// Surfaced by `ModelLifecyclePort::ensure_ready` and related operations.
// SDD reference: `.specs/veronex/history/inference-lifecycle-sod.md`.

#[derive(Debug, Clone, Error)]
pub enum LifecycleError {
    /// The hard cap on load duration was exceeded. `max_ms` matches the
    /// adapter's `LIFECYCLE_LOAD_TIMEOUT` constant (default 600_000).
    #[error("model load timed out after {elapsed_ms}ms (max {max_ms}ms)")]
    LoadTimeout { elapsed_ms: u64, max_ms: u64 },

    /// Probe is in flight but no progress observed for the stall window
    /// (default 60_000 ms). The probe is abandoned and the in-flight slot
    /// is released so the next caller can retry.
    #[error("model load stalled — no progress for {last_progress_ms}ms")]
    Stalled { last_progress_ms: u64 },

    /// Provider returned a non-success HTTP status or transport error.
    #[error("provider error during lifecycle: {0}")]
    ProviderError(String),

    /// Provider's circuit breaker is open. Caller should pick a different
    /// provider or back off.
    #[error("provider circuit breaker open")]
    CircuitOpen,

    /// VRAM accounting (VramPool) reports the model cannot fit.
    #[error("VRAM exhausted: available {available_vram_mb}MB, required {required_mb}MB")]
    ResourcesExhausted { available_vram_mb: u64, required_mb: u64 },
}

#[cfg(test)]
mod lifecycle_error_tests {
    use super::*;

    #[test]
    fn load_timeout_display_contains_durations() {
        let e = LifecycleError::LoadTimeout {
            elapsed_ms: 600_000,
            max_ms: 600_000,
        };
        let s = e.to_string();
        assert!(s.contains("600000ms"), "msg = {s}");
        assert!(s.contains("timed out"), "msg = {s}");
    }

    #[test]
    fn stalled_display_contains_progress_window() {
        let e = LifecycleError::Stalled { last_progress_ms: 60_000 };
        let s = e.to_string();
        assert!(s.contains("60000ms"), "msg = {s}");
        assert!(s.contains("stalled"), "msg = {s}");
    }

    #[test]
    fn provider_error_passes_through_inner_message() {
        let e = LifecycleError::ProviderError("ollama 502".into());
        assert_eq!(e.to_string(), "provider error during lifecycle: ollama 502");
    }

    #[test]
    fn circuit_open_has_actionable_message() {
        let e = LifecycleError::CircuitOpen;
        assert_eq!(e.to_string(), "provider circuit breaker open");
    }

    #[test]
    fn resources_exhausted_shows_required_vs_available() {
        let e = LifecycleError::ResourcesExhausted {
            available_vram_mb: 1_024,
            required_mb: 60_000,
        };
        let s = e.to_string();
        assert!(s.contains("1024MB"), "msg = {s}");
        assert!(s.contains("60000MB"), "msg = {s}");
    }
}
