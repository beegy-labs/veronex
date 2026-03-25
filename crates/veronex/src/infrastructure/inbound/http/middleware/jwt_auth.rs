use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::enums::AccountRole;
use crate::infrastructure::inbound::http::error::AppError;
use crate::infrastructure::inbound::http::state::AppState;
use crate::infrastructure::outbound::valkey_keys;

/// JWT claims payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject = account UUID.
    pub sub: Uuid,
    pub role: AccountRole,
    /// JWT ID — unique per session, used for Valkey revocation blocklist.
    pub jti: Uuid,
    pub exp: usize,
    /// Role-based permissions (e.g. ["dashboard_view", "provider_manage"]).
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Visible menu IDs (e.g. ["dashboard", "providers", "servers"]).
    #[serde(default)]
    pub menus: Vec<String>,
    /// Role name (e.g. "super", "viewer").
    #[serde(default)]
    pub role_name: String,
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
) -> Result<Response, AppError> {
    // Try Authorization header first (API clients), then fall back to HttpOnly cookie (web UI).
    let token_owned = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| extract_access_cookie(req.headers()))
        .ok_or_else(|| AppError::Unauthorized("missing or invalid Authorization header".into()))?;

    let key = DecodingKey::from_secret(state.jwt_secret.as_bytes());
    let token_data = decode::<Claims>(&token_owned, &key, &Validation::new(Algorithm::HS256))
        .map_err(|_| AppError::Unauthorized("invalid or expired token".into()))?;

    let claims = token_data.claims;

    // Check Valkey revocation blocklist (O(1)).
    // When Valkey is not configured, revocation checking is unavailable — acceptable
    // because no tokens can be revoked without a Valkey instance.
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let revoked_key = valkey_keys::revoked_jti(claims.jti);
        // Fail-closed: Valkey error must deny access, not silently allow a
        // potentially-revoked token through.
        let exists: bool = pool
            .exists(&revoked_key)
            .await
            .map_err(|e| {
                tracing::error!(jti = %claims.jti, "revocation check failed: {e}");
                AppError::ServiceUnavailable("token revocation check unavailable".into())
            })?;
        if exists {
            return Err(AppError::Unauthorized("token has been revoked".into()));
        }
    }

    // Non-blocking last_used update.
    {
        let repo = state.session_repo.clone();
        let jti = claims.jti;
        tokio::spawn(async move {
            if let Err(e) = repo.update_last_used(&jti).await {
                tracing::warn!(jti = %jti, "session last_used_at update failed: {e}");
            }
        });
    }

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

/// Extract the access token value from the `Cookie` header.
pub fn extract_access_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    for value in headers.get_all(axum::http::header::COOKIE) {
        if let Ok(s) = value.to_str() {
            for part in s.split(';') {
                let part = part.trim();
                if let Some(val) = part.strip_prefix("veronex_access_token=") {
                    let val = val.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Axum extractor that pulls [`Claims`] from extensions and asserts `role == Super`.
///
/// Returns `403 Forbidden` if the authenticated user is not a super-admin.
/// Used for role CRUD and other super-only operations.
pub struct RequireSuper(pub Claims);

impl<S> axum::extract::FromRequestParts<S> for RequireSuper
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let claims = parts
            .extensions
            .get::<Claims>()
            .cloned()
            .ok_or_else(|| AppError::Unauthorized("authentication required".into()))?;

        if claims.role != AccountRole::Super {
            return Err(AppError::Forbidden("super admin access required".into()));
        }

        Ok(RequireSuper(claims))
    }
}

/// Macro to generate permission-checking extractors.
///
/// Each generated struct works like `RequireSuper` but checks for a
/// specific permission string. Super-admin accounts bypass the check.
macro_rules! define_require_permission {
    ($name:ident, $perm:expr) => {
        pub struct $name(pub Claims);

        impl<S> axum::extract::FromRequestParts<S> for $name
        where
            S: Send + Sync,
        {
            type Rejection = AppError;

            async fn from_request_parts(
                parts: &mut axum::http::request::Parts,
                _state: &S,
            ) -> Result<Self, Self::Rejection> {
                let claims = parts
                    .extensions
                    .get::<Claims>()
                    .cloned()
                    .ok_or_else(|| AppError::Unauthorized("authentication required".into()))?;

                if claims.role == AccountRole::Super {
                    return Ok($name(claims));
                }

                if !claims.permissions.iter().any(|p| p == $perm) {
                    return Err(AppError::Forbidden(
                        format!("permission '{}' required", $perm),
                    ));
                }

                Ok($name(claims))
            }
        }
    };
}

define_require_permission!(RequireDashboardView,  "dashboard_view");
define_require_permission!(RequireApiTest,        "api_test");
define_require_permission!(RequireProviderManage, "provider_manage");
define_require_permission!(RequireKeyManage,      "key_manage");
define_require_permission!(RequireAccountManage,  "account_manage");
define_require_permission!(RequireAuditView,      "audit_view");
define_require_permission!(RequireSettingsManage, "settings_manage");
define_require_permission!(RequireRoleManage,     "role_manage");
define_require_permission!(RequireModelManage,    "model_manage");
