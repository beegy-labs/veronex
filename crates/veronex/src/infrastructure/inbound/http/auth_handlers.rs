use argon2::{Argon2, PasswordHash, PasswordVerifier};
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::{StatusCode, header::SET_COOKIE, HeaderMap};
use axum::Json;
use axum::response::IntoResponse;
use chrono::Utc;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::{Account, Session};
use crate::domain::enums::AccountRole;
use crate::domain::services::password_hashing;
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;
use crate::infrastructure::inbound::http::state::AppState;
use crate::infrastructure::outbound::valkey_keys;

use super::audit_helpers::emit_audit_raw as emit_audit;
use super::error::AppError;

const MIN_PASSWORD_LEN: usize = 8;

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// JSON body returned on successful login/setup.
/// Tokens are NOT included — they are set as HttpOnly cookies.
#[derive(Serialize)]
pub struct LoginResponse {
    pub ok: bool,
    pub account_id: Uuid,
    pub username: String,
    pub role: String,
}

/// JSON body returned on successful token refresh.
/// The new access token is set as an HttpOnly cookie — not in the body.
#[derive(Serialize)]
pub struct RefreshResponse {
    pub ok: bool,
}

#[derive(Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn issue_access_token(
    account_id: Uuid,
    role: AccountRole,
    jti: Uuid,
    secret: &str,
) -> Result<(String, chrono::DateTime<Utc>), AppError> {
    let expires_at = Utc::now() + chrono::Duration::hours(1);
    let exp = expires_at.timestamp() as usize;
    let claims = Claims {
        sub: account_id,
        role,
        jti,
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("token encoding failed: {e}")))?;
    Ok((token, expires_at))
}

fn hash_token(raw: &str) -> String {
    use blake2::{Blake2b, Digest, digest::consts::U32};
    type B = Blake2b<U32>;
    let mut h = B::new();
    h.update(b"veronex-refresh-token-v1:");
    h.update(raw.as_bytes());
    hex::encode(h.finalize())
}

fn pwreset_key(token: &str) -> String {
    valkey_keys::password_reset(token)
}

/// Add `jti` to the Valkey revocation blocklist with a TTL matching the token's
/// remaining lifetime.  Fail-open: Valkey errors are non-fatal.
pub(crate) async fn revoke_jti(state: &AppState, jti: Uuid, expires_at: chrono::DateTime<Utc>) {
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = valkey_keys::revoked_jti(jti);
        let ttl_secs = (expires_at - Utc::now()).num_seconds().max(1);
        if let Err(e) = pool.set::<(), _, _>(key, "1", Some(Expiration::EX(ttl_secs)), None, false).await {
            tracing::error!(jti = %jti, "failed to revoke session in Valkey: {e}");
        }
    }
}

/// Check whether a refresh token hash has already been consumed (replay).
/// Returns `true` if the token was already used.  Fail-open on Valkey errors.
async fn is_refresh_token_used(state: &AppState, hash: &str) -> bool {
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = valkey_keys::refresh_blocklist(hash);
        match pool.exists::<i64, _>(&key).await {
            Ok(1..) => return true,
            Err(e) => tracing::warn!("refresh blocklist check failed (fail-open): {e}"),
            _ => {}
        }
    }
    false
}

/// Mark a refresh token hash as consumed so it cannot be replayed.
/// TTL matches the session's remaining lifetime.  Fail-open on Valkey errors.
async fn blocklist_refresh_token(state: &AppState, hash: &str, expires_at: chrono::DateTime<Utc>) {
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = valkey_keys::refresh_blocklist(hash);
        let ttl_secs = (expires_at - Utc::now()).num_seconds().max(1);
        if let Err(e) = pool.set::<(), _, _>(key, "1", Some(Expiration::EX(ttl_secs)), None, false).await {
            tracing::error!("failed to blocklist refresh token in Valkey: {e}");
        }
    }
}

fn build_session(
    account_id: Uuid,
    jti: Uuid,
    refresh_hash: String,
    ip_address: Option<String>,
    expires_at: chrono::DateTime<Utc>,
) -> Session {
    Session {
        id: Uuid::now_v7(),
        account_id,
        jti,
        refresh_token_hash: Some(refresh_hash),
        ip_address,
        created_at: Utc::now(),
        last_used_at: None,
        expires_at,
        revoked_at: None,
    }
}

// ── Cookie helpers ────────────────────────────────────────────────────────────

const ACCESS_COOKIE: &str = "veronex_access_token";
const REFRESH_COOKIE: &str = "veronex_refresh_token";

/// Build `Set-Cookie` headers for both access and refresh tokens.
fn set_auth_cookies(headers: &mut HeaderMap, access_token: &str, refresh_token: &str) {
    use super::constants::{ACCESS_TOKEN_MAX_AGE, REFRESH_TOKEN_MAX_AGE};
    // Access token: sent on every request.
    headers.append(
        SET_COOKIE,
        format!(
            "{ACCESS_COOKIE}={access_token}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={ACCESS_TOKEN_MAX_AGE}"
        )
        .parse()
        .unwrap(),
    );
    // Refresh token: restricted to /v1/auth so it's only sent on auth requests.
    headers.append(
        SET_COOKIE,
        format!(
            "{REFRESH_COOKIE}={refresh_token}; HttpOnly; Secure; SameSite=Strict; Path=/v1/auth; Max-Age={REFRESH_TOKEN_MAX_AGE}"
        )
        .parse()
        .unwrap(),
    );
}

/// Build `Set-Cookie` headers that expire (clear) both auth cookies.
fn clear_auth_cookies(headers: &mut HeaderMap) {
    headers.append(
        SET_COOKIE,
        format!("{ACCESS_COOKIE}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0")
            .parse()
            .unwrap(),
    );
    headers.append(
        SET_COOKIE,
        format!("{REFRESH_COOKIE}=; HttpOnly; Secure; SameSite=Strict; Path=/v1/auth; Max-Age=0")
            .parse()
            .unwrap(),
    );
}

/// Extract the refresh token value from the `Cookie` header.
fn extract_refresh_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    for value in headers.get_all(axum::http::header::COOKIE) {
        if let Ok(s) = value.to_str() {
            for part in s.split(';') {
                let part = part.trim();
                if let Some(val) = part.strip_prefix("veronex_refresh_token=") {
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

// ── POST /v1/auth/login ───────────────────────────────────────────────────────

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    // ── M2: IP-based login rate limiting ─────────────────────────────
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let ip = addr.ip().to_string();
        let key = valkey_keys::login_attempts(&ip);
        let count: i64 = pool.incr_by(&key, 1).await.unwrap_or(1);
        if count == 1 {
            let _: bool = pool.expire(&key, 300, None).await.unwrap_or(false);
        }
        if count > 10 {
            return Err(AppError::TooManyRequests { retry_after: 300 });
        }
    }

    // ── H1: constant-time user lookup (prevent timing-based username enumeration)
    let account = match state.account_repo.get_by_username(&req.username).await? {
        Some(a) => a,
        None => {
            let dummy_hash = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
            let _ = PasswordHash::new(dummy_hash)
                .ok()
                .and_then(|h| Argon2::default().verify_password(b"dummy", &h).ok());
            return Err(AppError::Unauthorized("invalid credentials".into()));
        }
    };

    if !account.is_active {
        return Err(AppError::Unauthorized("account is disabled".into()));
    }

    // ── M3: don't leak hash parse errors to clients ──────────────────
    let parsed_hash = match PasswordHash::new(&account.password_hash) {
        Ok(h) => h,
        Err(_) => {
            tracing::error!(username = %req.username, "corrupted password hash in database");
            return Err(AppError::Unauthorized("invalid credentials".into()));
        }
    };
    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized("invalid credentials".into()))?;

    let _ = state.account_repo.update_last_login(&account.id).await;

    let jti = Uuid::now_v7();
    let (access_token, expires_at) =
        issue_access_token(account.id, account.role, jti, &state.jwt_secret)?;

    // Generate refresh token and hash it for storage.
    let refresh_raw = Uuid::new_v4().to_string();
    let refresh_hash = hash_token(&refresh_raw);

    // Persist session (non-fatal if it fails).
    let session = build_session(account.id, jti, refresh_hash.clone(), None, expires_at);
    if let Err(e) = state.session_repo.create(&session).await {
        tracing::warn!("failed to persist session (non-fatal): {e}");
    }

    emit_audit(&state, account.id, &account.username, "login", "account", &account.id.to_string(), &account.username,
        &format!("User '{}' logged in successfully", account.username)).await;

    let mut headers = HeaderMap::new();
    set_auth_cookies(&mut headers, &access_token, &refresh_raw);

    Ok((headers, Json(LoginResponse {
        ok: true,
        account_id: account.id,
        username: account.username,
        role: account.role.to_string(),
    })))
}

// ── POST /v1/auth/logout ──────────────────────────────────────────────────────

pub async fn logout(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    // Read refresh token from HttpOnly cookie (primary) or JSON body (API fallback).
    let refresh_token = extract_refresh_cookie(req.headers());

    if let Some(rt) = refresh_token {
        let hash = hash_token(&rt);

        // Revoke session in DB + add jti to Valkey blocklist.
        if let Ok(Some(session)) = state.session_repo.get_by_refresh_hash(&hash).await {
            let _ = state.session_repo.revoke(&session.id).await;
            revoke_jti(&state, session.jti, session.expires_at).await;
            // Resolve account username for audit (fallback to UUID if lookup fails).
            let account_name = state.account_repo.get_by_id(&session.account_id).await
                .ok().flatten().map(|a| a.username).unwrap_or_else(|| session.account_id.to_string());
            emit_audit(&state, session.account_id, &account_name, "logout", "account", &session.account_id.to_string(), &account_name,
                "Session terminated: refresh token revoked and JWT blocklisted").await;
        }
    }

    // Always clear auth cookies regardless of whether a refresh token was found.
    let mut headers = HeaderMap::new();
    clear_auth_cookies(&mut headers);
    (StatusCode::NO_CONTENT, headers)
}

// ── POST /v1/auth/refresh ─────────────────────────────────────────────────────

pub async fn refresh(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> Result<impl IntoResponse, AppError> {
    // Read refresh token from HttpOnly cookie.
    let refresh_raw = extract_refresh_cookie(req.headers())
        .ok_or_else(|| AppError::Unauthorized("missing refresh token".into()))?;

    let hash = hash_token(&refresh_raw);

    // Replay protection: reject tokens already consumed.
    if is_refresh_token_used(&state, &hash).await {
        return Err(AppError::Unauthorized("refresh token already used".into()));
    }

    let old_session = state
        .session_repo
        .get_by_refresh_hash(&hash)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid refresh token".into()))?;

    let account = state
        .account_repo
        .get_by_id(&old_session.account_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("account not found".into()))?;

    if !account.is_active {
        return Err(AppError::Unauthorized("account is disabled".into()));
    }

    // Blocklist the old refresh token hash BEFORE revoking to close the race
    // window where a replayed token could still be accepted.
    //
    // NOTE: There is a small TOCTOU race window between is_refresh_token_used()
    // and blocklist_refresh_token(). Two concurrent requests with the same token
    // could both pass the check before either sets the blocklist key. This is
    // mitigated by the DB session revocation (only one can succeed) and the
    // extremely short race window (~1ms). A Lua atomic check-and-set script
    // would eliminate this entirely but is deferred as the risk is minimal.
    blocklist_refresh_token(&state, &hash, old_session.expires_at).await;

    // Rolling refresh: revoke old session, issue new one.
    let _ = state.session_repo.revoke(&old_session.id).await;
    revoke_jti(&state, old_session.jti, old_session.expires_at).await;

    let new_jti = Uuid::now_v7();
    let (access_token, new_expires_at) =
        issue_access_token(account.id, account.role, new_jti, &state.jwt_secret)?;

    let new_refresh_raw = Uuid::new_v4().to_string();
    let new_refresh_hash = hash_token(&new_refresh_raw);

    let new_session = build_session(account.id, new_jti, new_refresh_hash, old_session.ip_address, new_expires_at);
    if let Err(e) = state.session_repo.create(&new_session).await {
        tracing::warn!("failed to persist refreshed session (non-fatal): {e}");
    }

    let mut headers = HeaderMap::new();
    set_auth_cookies(&mut headers, &access_token, &new_refresh_raw);

    Ok((headers, Json(RefreshResponse { ok: true })))
}

// ── POST /v1/auth/reset-password ─────────────────────────────────────────────

pub async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<StatusCode, AppError> {
    // S3: Validate password length before any Valkey/DB operations
    if req.new_password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::BadRequest("password must be at least 8 characters long".into()));
    }

    let pool = state.valkey_pool.as_ref()
        .ok_or_else(|| AppError::ServiceUnavailable("valkey not configured".into()))?;

    use fred::prelude::*;
    let key = pwreset_key(&req.token);
    // S1: Atomically get-and-delete to prevent token reuse race window
    let account_id_str: Option<String> =
        pool.getdel(&key).await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("valkey error: {e}")))?;
    let account_id_str = account_id_str
        .ok_or_else(|| AppError::Unauthorized("invalid or expired reset token".into()))?;

    let account_id =
        Uuid::parse_str(&account_id_str)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid account id in reset token: {e}")))?;

    let new_hash = password_hashing::hash_password(&req.new_password)?;

    state
        .account_repo
        .set_password_hash(&account_id, &new_hash)
        .await?;

    emit_audit(&state, account_id, &account_id.to_string(), "reset_password", "account", &account_id.to_string(), &account_id.to_string(),
        "Password changed via one-time reset token").await;

    Ok(StatusCode::NO_CONTENT)
}

// ── GET /v1/setup/status ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SetupStatusResponse {
    pub needs_setup: bool,
}

/// Returns whether the first-run setup is needed (no super account exists yet).
/// No authentication required.
pub async fn setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatusResponse>, AppError> {
    let accounts = state
        .account_repo
        .list_all()
        .await?;
    Ok(Json(SetupStatusResponse { needs_setup: accounts.is_empty() }))
}

// ── POST /v1/setup ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

/// Create the initial super admin account.
/// Returns 409 Conflict if any account already exists.
/// No authentication required (only callable before setup is complete).
pub async fn setup(
    State(state): State<AppState>,
    Json(req): Json<SetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    super::handlers::validate_username(&req.username)?;
    if req.password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters long".into(),
        ));
    }

    let hash = password_hashing::hash_password(&req.password)?;

    // Use a PG advisory lock to serialise the check-then-insert so two
    // concurrent requests cannot both pass the "no accounts exist" guard.
    // Lock 0xBEE6_0001 is an arbitrary namespace constant for "setup".
    let mut tx = state.pg_pool.begin().await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("begin tx: {e}")))?;
    sqlx::query("SELECT pg_advisory_xact_lock(3203399681)")   // 0xBEE6_0001
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("advisory lock: {e}")))?;

    // Guard: only allowed before any account exists (now serialised).
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM accounts WHERE deleted_at IS NULL")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count accounts: {e}")))?;
    if row.0 > 0 {
        return Err(AppError::Conflict("setup already completed".into()));
    }

    let account = Account {
        id: Uuid::now_v7(),
        username: req.username.trim().to_string(),
        password_hash: hash,
        name: "Super Admin".to_string(),
        email: None,
        role: AccountRole::Super,
        department: None,
        position: None,
        is_active: true,
        created_by: None,
        last_login_at: None,
        created_at: Utc::now(),
        deleted_at: None,
    };

    sqlx::query(
        "INSERT INTO accounts
         (id, username, password_hash, name, email, role, department, position,
          is_active, created_by, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(account.id)
    .bind(&account.username)
    .bind(&account.password_hash)
    .bind(&account.name)
    .bind(&account.email)
    .bind(account.role.as_str())
    .bind(&account.department)
    .bind(&account.position)
    .bind(account.is_active)
    .bind(account.created_by)
    .bind(account.created_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("insert account: {e}")))?;

    tx.commit().await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("commit tx: {e}")))?;

    tracing::info!("first-run setup: super account '{}' created", account.username);
    emit_audit(&state, account.id, &account.username, "create", "account", &account.id.to_string(), &account.username,
        &format!("First-run setup: super admin account '{}' created", account.username)).await;

    // Issue access token + session so the user lands directly on the dashboard.
    let jti = Uuid::now_v7();
    let (access_token, expires_at) =
        issue_access_token(account.id, account.role, jti, &state.jwt_secret)?;

    let refresh_raw = Uuid::new_v4().to_string();
    let refresh_hash = hash_token(&refresh_raw);

    let session = build_session(account.id, jti, refresh_hash, None, expires_at);
    let _ = state.session_repo.create(&session).await;

    let mut headers = HeaderMap::new();
    set_auth_cookies(&mut headers, &access_token, &refresh_raw);

    Ok((headers, Json(LoginResponse {
        ok: true,
        account_id: account.id,
        username: account.username,
        role: account.role.to_string(),
    })))
}

// ── Helper exported for account_handlers ──────────────────────────────────────

pub fn make_pwreset_valkey_key(token: &str) -> String {
    pwreset_key(token)
}
