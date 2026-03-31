pub mod audit;
pub mod ingest;
pub mod mcp;
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
    use proptest::prelude::*;

    /// Concrete examples for bearer token edge cases.
    #[test]
    fn extract_bearer_token_examples() {
        let hv = HeaderValue::from_static("Bearer my-secret-token");
        assert_eq!(extract_bearer_token(Some(&hv)), Some("my-secret-token"));
        assert_eq!(extract_bearer_token(None), None);
        let basic = HeaderValue::from_static("Basic dXNlcjpwYXNz");
        assert_eq!(extract_bearer_token(Some(&basic)), None);
        let no_space = HeaderValue::from_static("Bearertoken");
        assert_eq!(extract_bearer_token(Some(&no_space)), None);
    }

    #[test]
    fn default_hours_is_24() {
        assert_eq!(default_hours(), 24);
    }

    proptest! {
        /// Hours in valid range [1, 8760] always accepted.
        #[test]
        fn validate_hours_in_range_accepted(hours in 1u32..=8760) {
            prop_assert_eq!(validate_hours(hours), Ok(()));
        }

        /// Hours outside valid range always rejected.
        #[test]
        fn validate_hours_out_of_range_rejected(hours in 8761u32..=u32::MAX) {
            prop_assert_eq!(validate_hours(hours), Err(StatusCode::BAD_REQUEST));
        }

        #[test]
        fn validate_hours_zero_rejected(_ in 0u8..1) {
            prop_assert_eq!(validate_hours(0), Err(StatusCode::BAD_REQUEST));
        }

        /// success_rate is always in [0.0, 1.0] when success <= total.
        #[test]
        fn success_rate_bounded(total in 1u64..10000, success_pct in 0u64..=100) {
            let success = total * success_pct / 100;
            let rate = success_rate(total, success);
            prop_assert!(rate >= 0.0);
            prop_assert!(rate <= 1.0);
        }

        /// success_rate with zero total always returns 0.0.
        #[test]
        fn success_rate_zero_total_is_zero(success in 0u64..1000) {
            prop_assert_eq!(success_rate(0, success), 0.0);
        }

        /// success_rate is exact when total == success.
        #[test]
        fn success_rate_all_success_is_one(total in 1u64..10000) {
            prop_assert_eq!(success_rate(total, total), 1.0);
        }
    }
}
