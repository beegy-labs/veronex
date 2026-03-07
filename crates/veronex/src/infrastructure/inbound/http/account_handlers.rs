use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::{Account, ApiKey};
use crate::domain::enums::{AccountRole, KeyTier, KeyType};
use crate::domain::services::api_key_generator::generate_api_key;
use crate::domain::services::password_hashing;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSuper;
use crate::infrastructure::inbound::http::state::AppState;
use crate::infrastructure::outbound::valkey_keys;

use super::audit_helpers::emit_audit;
use super::error::AppError;

// ── Request / Response types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateAccountRequest {
    pub username: String,
    pub password: String,
    pub name: String,
    pub email: Option<String>,
    #[serde(default = "default_role")]
    pub role: AccountRole,
    pub department: Option<String>,
    pub position: Option<String>,
}

fn default_role() -> AccountRole {
    AccountRole::Admin
}

#[derive(Deserialize)]
pub struct UpdateAccountRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub department: Option<String>,
    pub position: Option<String>,
}

#[derive(Deserialize)]
pub struct SetActiveRequest {
    pub is_active: bool,
}

#[derive(Serialize)]
pub struct AccountSummary {
    pub id: Uuid,
    pub username: String,
    pub name: String,
    pub email: Option<String>,
    pub role: AccountRole,
    pub department: Option<String>,
    pub position: Option<String>,
    pub is_active: bool,
    pub last_login_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct CreateAccountResponse {
    pub id: Uuid,
    pub username: String,
    pub role: AccountRole,
    /// Plaintext test API key (shown once).
    pub test_api_key: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ResetLinkResponse {
    pub token: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn to_summary(a: Account) -> AccountSummary {
    AccountSummary {
        id: a.id,
        username: a.username,
        name: a.name,
        email: a.email,
        role: a.role,
        department: a.department,
        position: a.position,
        is_active: a.is_active,
        last_login_at: a.last_login_at,
        created_at: a.created_at,
    }
}

// ── GET /v1/accounts ──────────────────────────────────────────────────────────

pub async fn list_accounts(
    RequireSuper(_claims): RequireSuper,
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountSummary>>, AppError> {
    let accounts = state
        .account_repo
        .list_all()
        .await?;

    Ok(Json(accounts.into_iter().map(to_summary).collect()))
}

// ── POST /v1/accounts ─────────────────────────────────────────────────────────

pub async fn create_account(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<CreateAccountResponse>, AppError> {
    super::handlers::validate_username(&req.username)?;

    let password_hash = password_hashing::hash_password(&req.password)
        .map_err(AppError::Internal)?;
    let now = Utc::now();
    let account = Account {
        id: Uuid::now_v7(),
        username: req.username.clone(),
        password_hash,
        name: req.name.clone(),
        email: req.email.clone(),
        role: req.role,
        department: req.department.clone(),
        position: req.position.clone(),
        is_active: true,
        created_by: Some(claims.sub),
        last_login_at: None,
        created_at: now,
        deleted_at: None,
    };

    // Generate test API key for this account
    let (key_id, plaintext, key_hash, key_prefix) = generate_api_key();
    let test_key = ApiKey {
        id: key_id,
        key_hash,
        key_prefix,
        tenant_id: req.username.clone(),
        name: format!("{}-test", req.username),
        is_active: true,
        rate_limit_rpm: 0,
        rate_limit_tpm: 0,
        expires_at: None,
        created_at: now,
        deleted_at: None,
        key_type: KeyType::Test,
        tier: KeyTier::Paid,
    };

    // Create account in DB
    state
        .account_repo
        .create(&account)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                AppError::Conflict("username already exists".into())
            } else {
                AppError::Internal(e)
            }
        })?;

    // Create test API key (best-effort; account already exists)
    // In production this would be in a transaction, but sqlx PgPool supports it.
    let _ = state.api_key_repo.create(&test_key).await;

    emit_audit(&state, &claims, "create", "account", &account.id.to_string(), &req.username,
        &format!("Account '{}' (role: {}) created with auto-generated test API key",
            req.username, req.role)).await;

    Ok(Json(CreateAccountResponse {
        id: account.id,
        username: req.username,
        role: req.role,
        test_api_key: plaintext,
        created_at: now,
    }))
}

// ── PATCH /v1/accounts/{id} ───────────────────────────────────────────────────

pub async fn update_account(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<StatusCode, AppError> {
    let uuid = super::handlers::parse_uuid(&id)?;

    let mut account = state
        .account_repo
        .get_by_id(&uuid)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("account {id} not found")))?;

    if let Some(name) = req.name {
        account.name = name;
    }
    account.email = req.email.or(account.email);
    account.department = req.department.or(account.department);
    account.position = req.position.or(account.position);

    state
        .account_repo
        .update(&account)
        .await?;

    emit_audit(&state, &claims, "update", "account", &id, &account.username,
        &format!("Account '{}' ({}) profile updated (name/email/department/position)",
            account.username, id)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/accounts/{id} ──────────────────────────────────────────────────

pub async fn delete_account(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let uuid = super::handlers::parse_uuid(&id)?;

    let account = state
        .account_repo
        .get_by_id(&uuid)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("account {id} not found")))?;

    // Cascade: soft-delete account + all its API keys in a single transaction
    let keys_deleted = state
        .account_repo
        .soft_delete_cascade(&uuid, &account.username)
        .await?;

    emit_audit(&state, &claims, "delete", "account", &id, &account.username,
        &format!("Account '{}' ({}) soft-deleted (login disabled, data retained, {} API key(s) revoked)",
            account.username, id, keys_deleted)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── PATCH /v1/accounts/{id}/active ────────────────────────────────────────────

pub async fn set_account_active(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SetActiveRequest>,
) -> Result<StatusCode, AppError> {
    let uuid = super::handlers::parse_uuid(&id)?;

    state
        .account_repo
        .set_active(&uuid, req.is_active)
        .await?;

    emit_audit(&state, &claims, "update", "account", &id, &id,
        &format!("Account {} is_active set to {} (login {})",
            id, req.is_active, if req.is_active { "enabled" } else { "disabled" })).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── GET /v1/accounts/{id}/sessions ────────────────────────────────────────────

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub ip_address: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
    pub expires_at: chrono::DateTime<Utc>,
}

pub async fn list_account_sessions(
    RequireSuper(_claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummary>>, AppError> {
    let uuid = super::handlers::parse_uuid(&id)?;

    let sessions = state
        .session_repo
        .list_active(&uuid)
        .await?;

    let summaries = sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: s.id,
            ip_address: s.ip_address,
            created_at: s.created_at,
            last_used_at: s.last_used_at,
            expires_at: s.expires_at,
        })
        .collect();

    Ok(Json(summaries))
}

// ── DELETE /v1/sessions/{session_id} ──────────────────────────────────────────

pub async fn revoke_session(
    RequireSuper(claims): RequireSuper,
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let uuid = super::handlers::parse_uuid(&session_id)?;
    // Fetch the session first to get jti + expires_at for Valkey blocklist.
    let session = state
        .session_repo
        .get_by_id(&uuid)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("session {session_id} not found")))?;
    state
        .session_repo
        .revoke(&uuid)
        .await?;
    // Add JTI to Valkey blocklist so the JWT is rejected immediately.
    super::auth_handlers::revoke_jti(&state, session.jti, session.expires_at).await;
    emit_audit(&state, &claims, "delete", "session", &session_id, &session_id,
        &format!("Session {} manually revoked by admin", session_id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/accounts/{id}/sessions ─────────────────────────────────────────

pub async fn revoke_all_account_sessions(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let uuid = super::handlers::parse_uuid(&id)?;
    // Fetch active sessions before revoking so we can blocklist each JTI.
    let sessions = state
        .session_repo
        .list_active(&uuid)
        .await?;
    state
        .session_repo
        .revoke_all_for_account(&uuid)
        .await?;
    // Add each JTI to the Valkey blocklist so JWTs are rejected immediately.
    for session in &sessions {
        super::auth_handlers::revoke_jti(&state, session.jti, session.expires_at).await;
    }
    emit_audit(&state, &claims, "delete", "session", &id, &format!("all_sessions:{id}"),
        &format!("All active sessions for account {} force-revoked by admin ({} session(s) blocklisted)", id, sessions.len())).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── POST /v1/accounts/{id}/reset-link ─────────────────────────────────────────

pub async fn create_reset_link(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ResetLinkResponse>, AppError> {
    let uuid = super::handlers::parse_uuid(&id)?;

    // Ensure account exists
    let account = state
        .account_repo
        .get_by_id(&uuid)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("account {id} not found")))?;

    let token = Uuid::new_v4().to_string();

    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = valkey_keys::password_reset(&token);
        let _: Result<(), _> = pool
            .set(key, uuid.to_string(), Some(fred::types::Expiration::EX(24 * 3600)), None, false)
            .await;
    }

    emit_audit(&state, &claims, "reset_password", "account", &id, &account.username,
        &format!("Password reset link generated for account '{}' ({}); token valid 24h",
            account.username, id)).await;

    Ok(Json(ResetLinkResponse { token }))
}
