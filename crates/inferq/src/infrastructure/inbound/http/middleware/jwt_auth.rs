use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::inbound::http::state::AppState;

/// JWT claims payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject = account UUID.
    pub sub: Uuid,
    pub role: String,
    /// JWT ID — unique per session, used for Valkey revocation blocklist.
    pub jti: Uuid,
    pub exp: usize,
}

/// Axum middleware that validates `Authorization: Bearer <token>` and inserts
/// the decoded [`Claims`] into request extensions.
///
/// Additional checks:
/// - Valkey `veronex:revoked:{jti}` key presence → 401 (token revoked)
/// - `session_repo.update_last_used(&claims.jti)` called asynchronously (non-blocking)
pub async fn jwt_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let key = DecodingKey::from_secret(state.jwt_secret.as_bytes());
    let token_data = decode::<Claims>(token, &key, &Validation::default())
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let claims = token_data.claims;

    // Check Valkey revocation blocklist (O(1)).
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let revoked_key = format!("veronex:revoked:{}", claims.jti);
        let exists: bool = pool
            .exists(&revoked_key)
            .await
            .unwrap_or(false);
        if exists {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Non-blocking last_used update.
    {
        let repo = state.session_repo.clone();
        let jti = claims.jti;
        tokio::spawn(async move {
            let _ = repo.update_last_used(&jti).await;
        });
    }

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

/// Axum extractor that pulls [`Claims`] from extensions and asserts `role == "super"`.
///
/// Returns `403 Forbidden` if the authenticated user is not a super-admin.
pub struct RequireSuper(pub Claims);

impl<S> axum::extract::FromRequestParts<S> for RequireSuper
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let claims = parts
            .extensions
            .get::<Claims>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if claims.role != "super" {
            return Err(StatusCode::FORBIDDEN);
        }

        Ok(RequireSuper(claims))
    }
}
