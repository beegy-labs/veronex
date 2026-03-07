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

    // ── Conflict ─────────────────────────────────────────────────────────
    #[error("conflict: {0}")]
    Conflict(String),
}
