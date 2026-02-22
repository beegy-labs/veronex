use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use blake2::{Blake2b, Digest, digest::consts::U32};

type Blake2b256 = Blake2b<U32>;
use chrono::Utc;

use crate::infrastructure::inbound::http::state::AppState;

const EXCLUDED_PATHS: &[&str] = &["/health", "/readyz"];

/// Axum middleware that validates X-API-Key header against the database.
///
/// Skips health/readiness endpoints. On success, inserts the `ApiKey` entity
/// into request extensions for downstream handlers/middleware.
pub async fn api_key_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if EXCLUDED_PATHS.iter().any(|p| path.starts_with(p)) {
        return Ok(next.run(req).await);
    }

    let raw_key = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let mut hasher = Blake2b256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let api_key = state
        .api_key_repo
        .get_by_hash(&key_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !api_key.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(expires) = api_key.expires_at {
        if expires < Utc::now() {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    req.extensions_mut().insert(api_key);
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_paths_contains_health() {
        assert!(EXCLUDED_PATHS.contains(&"/health"));
        assert!(EXCLUDED_PATHS.contains(&"/readyz"));
    }

    #[test]
    fn excluded_path_matching() {
        let path = "/health";
        assert!(EXCLUDED_PATHS.iter().any(|p| path.starts_with(p)));

        let path = "/readyz";
        assert!(EXCLUDED_PATHS.iter().any(|p| path.starts_with(p)));

        let path = "/v1/inference";
        assert!(!EXCLUDED_PATHS.iter().any(|p| path.starts_with(p)));

        let path = "/v1/keys";
        assert!(!EXCLUDED_PATHS.iter().any(|p| path.starts_with(p)));
    }
}
