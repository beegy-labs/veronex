pub mod audit;
pub mod ingest;
pub mod metrics;
pub mod performance;
pub mod usage;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use serde::Deserialize;

use crate::state::AppState;

/// Shared query parameter with hours filter (default 24h).
#[derive(Deserialize)]
pub struct HoursQuery {
    #[serde(default = "default_hours")]
    pub hours: u32,
}

fn default_hours() -> u32 {
    24
}

/// Validate hours parameter: must be 1..=8760.
pub fn validate_hours(hours: u32) -> Result<(), StatusCode> {
    if hours == 0 || hours > 8760 {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

/// Format an `OffsetDateTime` as RFC 3339, returning empty string on error.
pub fn format_rfc3339(dt: time::OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Compute success rate (0.0 when no requests).
pub fn success_rate(total: u64, success: u64) -> f64 {
    if total > 0 {
        success as f64 / total as f64
    } else {
        0.0
    }
}

/// Log a ClickHouse query failure and return 500.
pub fn ch_query_error(e: impl std::fmt::Display, context: &str) -> StatusCode {
    tracing::warn!("{context}: {e}");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Middleware that validates `Authorization: Bearer {ANALYTICS_SECRET}`.
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if token != state.analytics_secret {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

/// Extract bearer token from an Authorization header value.
///
/// Returns `None` if the header is missing, not valid UTF-8, or does not
/// start with `"Bearer "`.
#[cfg(test)]
fn extract_bearer_token(header: Option<&axum::http::HeaderValue>) -> Option<&str> {
    header
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extract_valid_bearer_token() {
        let hv = HeaderValue::from_static("Bearer my-secret-token");
        assert_eq!(extract_bearer_token(Some(&hv)), Some("my-secret-token"));
    }

    #[test]
    fn extract_missing_header() {
        assert_eq!(extract_bearer_token(None), None);
    }

    #[test]
    fn extract_wrong_scheme() {
        let hv = HeaderValue::from_static("Basic dXNlcjpwYXNz");
        assert_eq!(extract_bearer_token(Some(&hv)), None);
    }

    #[test]
    fn extract_bearer_no_space() {
        let hv = HeaderValue::from_static("Bearertoken");
        assert_eq!(extract_bearer_token(Some(&hv)), None);
    }

    #[test]
    fn extract_empty_token() {
        let hv = HeaderValue::from_static("Bearer ");
        assert_eq!(extract_bearer_token(Some(&hv)), Some(""));
    }

    #[test]
    fn extract_token_with_spaces() {
        let hv = HeaderValue::from_static("Bearer abc def");
        assert_eq!(extract_bearer_token(Some(&hv)), Some("abc def"));
    }

    #[test]
    fn default_hours_is_24() {
        assert_eq!(default_hours(), 24);
    }

    #[test]
    fn validate_hours_zero_rejected() {
        assert_eq!(validate_hours(0), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn validate_hours_over_8760_rejected() {
        assert_eq!(validate_hours(8761), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn validate_hours_boundary_1_accepted() {
        assert_eq!(validate_hours(1), Ok(()));
    }

    #[test]
    fn validate_hours_boundary_8760_accepted() {
        assert_eq!(validate_hours(8760), Ok(()));
    }

    #[test]
    fn success_rate_normal() {
        let rate = success_rate(100, 95);
        assert!((rate - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn success_rate_zero_requests() {
        assert_eq!(success_rate(0, 0), 0.0);
    }

    #[test]
    fn success_rate_all_success() {
        assert_eq!(success_rate(50, 50), 1.0);
    }

    #[test]
    fn success_rate_none_success() {
        assert_eq!(success_rate(10, 0), 0.0);
    }
}
