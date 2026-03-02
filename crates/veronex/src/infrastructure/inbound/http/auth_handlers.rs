use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::audit_port::AuditEvent;
use crate::domain::entities::{Account, Session};
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;
use crate::infrastructure::inbound::http::state::AppState;

async fn emit_audit(
    state: &AppState,
    account_id: Uuid,
    account_name: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    resource_name: &str,
    details: &str,
) {
    if let Some(ref port) = state.audit_port {
        port.record(AuditEvent {
            event_time: Utc::now(),
            account_id,
            account_name: account_name.to_string(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            resource_name: resource_name.to_string(),
            ip_address: None,
            details: Some(details.to_string()),
        })
        .await;
    }
}

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub account_id: Uuid,
    pub username: String,
    pub role: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub token_type: String,
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn issue_access_token(
    account_id: Uuid,
    role: &str,
    jti: Uuid,
    secret: &str,
) -> Result<(String, chrono::DateTime<Utc>), StatusCode> {
    let expires_at = Utc::now() + chrono::Duration::hours(1);
    let exp = expires_at.timestamp() as usize;
    let claims = Claims {
        sub: account_id,
        role: role.to_string(),
        jti,
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((token, expires_at))
}

fn hash_token(raw: &str) -> String {
    use blake2::{Blake2b, Digest, digest::consts::U32};
    type B = Blake2b<U32>;
    let mut h = B::new();
    h.update(raw.as_bytes());
    hex::encode(h.finalize())
}

fn pwreset_key(token: &str) -> String {
    format!("veronex:pwreset:{token}")
}

/// Add `jti` to the Valkey revocation blocklist with a TTL matching the token's
/// remaining lifetime.  Fail-open: Valkey errors are non-fatal.
async fn revoke_jti(state: &AppState, jti: Uuid, expires_at: chrono::DateTime<Utc>) {
    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = format!("veronex:revoked:{jti}");
        let ttl_secs = (expires_at - Utc::now()).num_seconds().max(1);
        let _: Result<(), _> =
            pool.set(key, "1", Some(Expiration::EX(ttl_secs)), None, false).await;
    }
}

// ── POST /v1/auth/login ───────────────────────────────────────────────────────

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let account = state
        .account_repo
        .get_by_username(&req.username)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !account.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let parsed_hash =
        PasswordHash::new(&account.password_hash).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed_hash)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let _ = state.account_repo.update_last_login(&account.id).await;

    let jti = Uuid::now_v7();
    let (access_token, expires_at) =
        issue_access_token(account.id, &account.role, jti, &state.jwt_secret)?;

    // Generate refresh token and hash it for storage.
    let refresh_raw = Uuid::new_v4().to_string();
    let refresh_hash = hash_token(&refresh_raw);

    // Persist session (non-fatal if it fails).
    let session = Session {
        id: Uuid::now_v7(),
        account_id: account.id,
        jti,
        refresh_token_hash: Some(refresh_hash.clone()),
        ip_address: None,
        created_at: Utc::now(),
        last_used_at: None,
        expires_at,
        revoked_at: None,
    };
    if let Err(e) = state.session_repo.create(&session).await {
        tracing::warn!("failed to persist session (non-fatal): {e}");
    }

    emit_audit(&state, account.id, &account.username, "login", "account", &account.id.to_string(), &account.username,
        &format!("User '{}' logged in successfully", account.username)).await;

    Ok(Json(LoginResponse {
        access_token,
        token_type: "Bearer".to_string(),
        account_id: account.id,
        username: account.username,
        role: account.role,
        refresh_token: refresh_raw,
    }))
}

// ── POST /v1/auth/logout ──────────────────────────────────────────────────────

pub async fn logout(
    State(state): State<AppState>,
    Json(req): Json<LogoutRequest>,
) -> StatusCode {
    let hash = hash_token(&req.refresh_token);

    // Revoke session in DB + add jti to Valkey blocklist.
    if let Ok(Some(session)) = state.session_repo.get_by_refresh_hash(&hash).await {
        let _ = state.session_repo.revoke(&session.id).await;
        revoke_jti(&state, session.jti, session.expires_at).await;
        emit_audit(&state, session.account_id, &session.account_id.to_string(), "logout", "account", &session.account_id.to_string(), &session.account_id.to_string(),
            "Session terminated: refresh token revoked and JWT blocklisted").await;
    }

    StatusCode::NO_CONTENT
}

// ── POST /v1/auth/refresh ─────────────────────────────────────────────────────

pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<RefreshResponse>, StatusCode> {
    let hash = hash_token(&req.refresh_token);

    let old_session = state
        .session_repo
        .get_by_refresh_hash(&hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let account = state
        .account_repo
        .get_by_id(&old_session.account_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !account.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Rolling refresh: revoke old session, issue new one.
    let _ = state.session_repo.revoke(&old_session.id).await;
    revoke_jti(&state, old_session.jti, old_session.expires_at).await;

    let new_jti = Uuid::now_v7();
    let (access_token, new_expires_at) =
        issue_access_token(account.id, &account.role, new_jti, &state.jwt_secret)?;

    let new_refresh_raw = Uuid::new_v4().to_string();
    let new_refresh_hash = hash_token(&new_refresh_raw);

    let new_session = Session {
        id: Uuid::now_v7(),
        account_id: account.id,
        jti: new_jti,
        refresh_token_hash: Some(new_refresh_hash),
        ip_address: old_session.ip_address,
        created_at: Utc::now(),
        last_used_at: None,
        expires_at: new_expires_at,
        revoked_at: None,
    };
    if let Err(e) = state.session_repo.create(&new_session).await {
        tracing::warn!("failed to persist refreshed session (non-fatal): {e}");
    }

    Ok(Json(RefreshResponse {
        access_token,
        token_type: "Bearer".to_string(),
    }))
}

// ── POST /v1/auth/reset-password ─────────────────────────────────────────────

pub async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<StatusCode, StatusCode> {
    let pool = state.valkey_pool.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    use fred::prelude::*;
    let key = pwreset_key(&req.token);
    let account_id_str: Option<String> =
        pool.get(&key).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let account_id_str = account_id_str.ok_or(StatusCode::UNAUTHORIZED)?;

    let _: Result<(), _> = pool.del(&key).await;

    let account_id =
        Uuid::parse_str(&account_id_str).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let salt = SaltString::generate(&mut OsRng);
    let new_hash = Argon2::default()
        .hash_password(req.new_password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

    state
        .account_repo
        .set_password_hash(&account_id, &new_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
) -> Result<Json<SetupStatusResponse>, StatusCode> {
    let accounts = state
        .account_repo
        .list_all()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
) -> Result<Json<LoginResponse>, StatusCode> {
    // Guard: only allowed before any account exists.
    let accounts = state
        .account_repo
        .list_all()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !accounts.is_empty() {
        return Err(StatusCode::CONFLICT);
    }

    if req.username.trim().is_empty() || req.password.len() < 8 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();

    let account = Account {
        id: Uuid::now_v7(),
        username: req.username.trim().to_string(),
        password_hash: hash,
        name: "Super Admin".to_string(),
        email: None,
        role: "super".to_string(),
        department: None,
        position: None,
        is_active: true,
        created_by: None,
        last_login_at: None,
        created_at: Utc::now(),
        deleted_at: None,
    };
    state
        .account_repo
        .create(&account)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!("first-run setup: super account '{}' created", account.username);
    emit_audit(&state, account.id, &account.username, "create", "account", &account.id.to_string(), &account.username,
        &format!("First-run setup: super admin account '{}' created", account.username)).await;

    // Issue access token + session so the user lands directly on the dashboard.
    let jti = Uuid::now_v7();
    let (access_token, expires_at) =
        issue_access_token(account.id, &account.role, jti, &state.jwt_secret)?;

    let refresh_raw = Uuid::new_v4().to_string();
    let refresh_hash = hash_token(&refresh_raw);

    let session = Session {
        id: Uuid::now_v7(),
        account_id: account.id,
        jti,
        refresh_token_hash: Some(refresh_hash),
        ip_address: None,
        created_at: Utc::now(),
        last_used_at: None,
        expires_at,
        revoked_at: None,
    };
    let _ = state.session_repo.create(&session).await;

    Ok(Json(LoginResponse {
        access_token,
        token_type: "Bearer".to_string(),
        account_id: account.id,
        username: account.username,
        role: account.role,
        refresh_token: refresh_raw,
    }))
}

// ── Helper exported for account_handlers ──────────────────────────────────────

pub fn make_pwreset_valkey_key(token: &str) -> String {
    pwreset_key(token)
}
