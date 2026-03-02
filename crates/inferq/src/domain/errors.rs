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

    #[error("forbidden: {0}")]
    Forbidden(String),

    // ── Rate limiting ─────────────────────────────────────────────────────
    #[error("rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    // ── Backend / inference ───────────────────────────────────────────────
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

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

impl DomainError {
    /// HTTP status code for this error variant.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Validation(_)         => 400,
            Self::Unauthorized(_)       => 401,
            Self::Forbidden(_)          => 403,
            Self::NotFound(_)           => 404,
            Self::Conflict(_)           => 409,
            Self::RateLimitExceeded(_)  => 429,
            Self::BackendUnavailable(_) | Self::InferenceFailed(_) => 502,
            Self::QueueFull(_)          => 503,
            Self::Configuration(_)      => 500,
        }
    }
}
