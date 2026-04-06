use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

use crate::domain::entities::ApiKey;
use crate::domain::services::api_key_generator::hash_api_key;
use crate::infrastructure::inbound::http::error::AppError;
use crate::infrastructure::inbound::http::middleware::jwt_auth::{Claims, extract_access_cookie};
use crate::infrastructure::inbound::http::state::AppState;

/// Caller identity for inference endpoints.
///
/// - `ApiKey`  — authenticated via `X-API-Key` / `Authorization: Bearer <vnx_…>` header.
///              Rate limiting applies; `source = Api`.
/// - `Session` — authenticated via JWT session (cookie or Bearer token).
///              Caller must hold `api_test` permission.
///              Rate limiting is skipped; `source = Test`.
#[derive(Clone, Debug)]
pub enum InferCaller {
    ApiKey(ApiKey),
    Session(Claims),
}

impl InferCaller {
    pub fn api_key_id(&self) -> Option<uuid::Uuid> {
        match self {
            InferCaller::ApiKey(k) => Some(k.id),
            InferCaller::Session(_) => None,
        }
    }

    pub fn account_id(&self) -> Option<uuid::Uuid> {
        match self {
            InferCaller::ApiKey(_) => None,
            InferCaller::Session(c) => Some(c.sub),
        }
    }

    pub fn key_tier(&self) -> Option<crate::domain::enums::KeyTier> {
        match self {
            InferCaller::ApiKey(k) => Some(k.tier),
            InferCaller::Session(_) => None,
        }
    }

    pub fn source(&self) -> crate::domain::enums::JobSource {
        match self {
            InferCaller::ApiKey(_) => crate::domain::enums::JobSource::Api,
            InferCaller::Session(_) => crate::domain::enums::JobSource::Test,
        }
    }

}

const EXCLUDED_PATHS: &[&str] = &["/health", "/readyz"];

/// Middleware that accepts either an API key OR a JWT session with `api_test` permission.
///
/// Precedence:
/// 1. API key header (`X-API-Key`, `Authorization: Bearer <vnx_…>`, `x-goog-api-key`)
/// 2. JWT session (cookie or `Authorization: Bearer <jwt>`)
///
/// JWT fallback requires `api_test` permission → 403 if missing.
/// Neither present → 401.
pub async fn infer_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = req.uri().path();
    if EXCLUDED_PATHS.contains(&path) {
        return Ok(next.run(req).await);
    }

    let headers = req.headers();

    // ── Try API key first ──────────────────────────────────────────────────────
    let api_key_raw = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("Authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                // Exclude JWT tokens (they don't start with our key prefix)
                .filter(|v| v.starts_with("vnx_"))
        })
        .or_else(|| {
            headers
                .get("x-goog-api-key")
                .and_then(|v| v.to_str().ok())
        });

    if let Some(raw) = api_key_raw {
        if raw.is_empty() {
            return Err(AppError::Unauthorized("missing API key".into()));
        }
        let key_hash = hash_api_key(raw);
        let api_key = state
            .api_key_repo
            .get_by_hash(&key_hash)
            .await?
            .ok_or_else(|| AppError::Unauthorized("invalid API key".into()))?;

        if !api_key.is_active {
            return Err(AppError::Unauthorized("API key is disabled".into()));
        }
        if let Some(expires) = api_key.expires_at
            && expires < chrono::Utc::now()
        {
            return Err(AppError::Unauthorized("API key has expired".into()));
        }
        if api_key.key_type.is_test() {
            return Err(AppError::Forbidden(
                "test API keys are not permitted for API access".into(),
            ));
        }

        req.extensions_mut().insert(InferCaller::ApiKey(api_key));
        return Ok(next.run(req).await);
    }

    // ── Fall back to JWT session ───────────────────────────────────────────────
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| extract_access_cookie(req.headers()))
        .ok_or_else(|| AppError::Unauthorized("missing API key or session token".into()))?;

    let key = DecodingKey::from_secret(state.jwt_secret.as_bytes());
    let claims = decode::<Claims>(&token, &key, &Validation::new(Algorithm::HS256))
        .map_err(|_| AppError::Unauthorized("invalid or expired token".into()))?
        .claims;

    // Revocation check
    if let Some(ref pool) = state.valkey_pool {
        use crate::infrastructure::outbound::valkey_keys;
        use fred::interfaces::KeysInterface as _;
        let revoked_key = valkey_keys::revoked_jti(claims.jti);
        let is_revoked: bool = match pool.next().exists(revoked_key).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, "JWT revocation check: Valkey unavailable — treating token as unrevoked");
                false
            }
        };
        if is_revoked {
            return Err(AppError::Unauthorized("session has been revoked".into()));
        }
    }

    // api_test permission required
    if !claims.permissions.iter().any(|p| p == "api_test") {
        return Err(AppError::Forbidden(
            "api_test permission required to call inference endpoints".into(),
        ));
    }

    req.extensions_mut().insert(InferCaller::Session(claims));
    Ok(next.run(req).await)
}
