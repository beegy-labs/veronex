use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Unified error type for all HTTP handlers.
///
/// Implements `IntoResponse` so handlers can return `Result<T, AppError>`.
/// Every variant produces a JSON body `{"error": "..."}` with the appropriate
/// HTTP status code, ensuring clients always receive structured errors.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("too many requests")]
    TooManyRequests { retry_after: u64 },

    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("unprocessable entity: {0}")]
    UnprocessableEntity(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),

    #[error("bad gateway: {0}")]
    BadGateway(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            Self::TooManyRequests { retry_after } => {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("Retry-After", retry_after.to_string())],
                    Json(json!({"error": "too many requests", "retry_after": retry_after})),
                )
                    .into_response();
            }
            Self::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            Self::UnprocessableEntity(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            Self::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg.clone()),
            Self::BadGateway(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            Self::Internal(e) => {
                tracing::error!("internal: {e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                )
            }
        };

        (status, Json(json!({"error": message}))).into_response()
    }
}

/// Convenience constructor for database-related internal errors.
///
/// Logs the underlying error at ERROR level and wraps it as `AppError::Internal`
/// with the shared `ERR_DATABASE` message.
pub fn db_error(e: impl std::fmt::Display) -> AppError {
    tracing::error!(error = %e, "{}", super::constants::ERR_DATABASE);
    AppError::Internal(anyhow::anyhow!(super::constants::ERR_DATABASE))
}

/// Build a JSON error response tuple `(StatusCode, Json)` for handlers that
/// return `impl IntoResponse` instead of `Result<T, AppError>`.
///
/// Produces `{"error": "<message>"}` — the same shape as `AppError::into_response`.
pub fn error_json(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({"error": message.into()})))
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        db_error(e)
    }
}

impl From<crate::domain::errors::DomainError> for AppError {
    fn from(e: crate::domain::errors::DomainError) -> Self {
        use crate::domain::errors::DomainError;
        match e {
            DomainError::Validation(msg) => Self::BadRequest(msg),
            DomainError::NotFound(msg) => Self::NotFound(msg),
            DomainError::Unauthorized(msg) => Self::Unauthorized(msg),
            DomainError::ExpiredKey(msg) => Self::Unauthorized(msg),
            DomainError::InvalidKey => Self::Unauthorized("invalid API key".into()),
            DomainError::Forbidden(msg) => Self::Forbidden(msg),
            DomainError::Conflict(msg) => Self::Conflict(msg),
            DomainError::RateLimitExceeded(_) => Self::TooManyRequests { retry_after: 60 },
            DomainError::ProviderUnavailable(msg) | DomainError::InferenceFailed(msg) => {
                Self::ServiceUnavailable(msg)
            }
            DomainError::QueueFull(msg) => Self::ServiceUnavailable(msg),
            DomainError::Configuration(msg) => Self::Internal(anyhow::anyhow!(msg)),
            DomainError::Internal(e) => Self::Internal(e),
        }
    }
}
