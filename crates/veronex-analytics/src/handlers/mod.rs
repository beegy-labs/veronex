pub mod audit;
pub mod ingest;
pub mod metrics;
pub mod performance;
pub mod usage;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

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
