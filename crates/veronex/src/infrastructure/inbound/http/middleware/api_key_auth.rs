use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use chrono::Utc;

use crate::domain::services::api_key_generator::hash_api_key;
use crate::infrastructure::inbound::http::error::AppError;
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
) -> Result<Response, AppError> {
    let path = req.uri().path();
    if EXCLUDED_PATHS.contains(&path) {
        return Ok(next.run(req).await);
    }

    // Accept X-API-Key, Authorization: Bearer (OpenAI-compatible), or x-goog-api-key (Gemini CLI).
    let headers = req.headers();
    let raw_key = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("Authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        .or_else(|| {
            headers
                .get("x-goog-api-key")
                .and_then(|v| v.to_str().ok())
        })
        .ok_or_else(|| AppError::Unauthorized("missing API key".into()))?;

    if raw_key.is_empty() {
        return Err(AppError::Unauthorized("missing API key".into()));
    }

    let key_hash = hash_api_key(raw_key);

    let api_key = state
        .api_key_repo
        .get_by_hash(&key_hash)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid API key".into()))?;

    if !api_key.is_active {
        return Err(AppError::Unauthorized("API key is disabled".into()));
    }
    if let Some(expires) = api_key.expires_at
        && expires < Utc::now() {
            return Err(AppError::Unauthorized("API key has expired".into()));
        }

    // RH1: Test keys must not access production inference endpoints.
    // Test routes are JWT-protected, so test API keys are blocked entirely here.
    if api_key.key_type.is_test() {
        return Err(AppError::Forbidden(
            "test API keys are not permitted for API access".into(),
        ));
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
        assert!(EXCLUDED_PATHS.iter().any(|p| path == *p));

        let path = "/readyz";
        assert!(EXCLUDED_PATHS.iter().any(|p| path == *p));

        let path = "/v1/inference";
        assert!(!EXCLUDED_PATHS.iter().any(|p| path == *p));

        let path = "/v1/keys";
        assert!(!EXCLUDED_PATHS.iter().any(|p| path == *p));

        // Exact match prevents prefix bypass (e.g., /healthXYZ)
        let path = "/healthXYZ";
        assert!(!EXCLUDED_PATHS.iter().any(|p| path == *p));
    }
}
