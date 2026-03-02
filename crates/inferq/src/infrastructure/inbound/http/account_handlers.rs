use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::ports::outbound::audit_port::AuditEvent;
use crate::domain::entities::{Account, ApiKey};
use crate::domain::services::api_key_generator::generate_api_key;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireSuper;
use crate::infrastructure::inbound::http::state::AppState;

// ── Request / Response types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateAccountRequest {
    pub username: String,
    pub password: String,
    pub name: String,
    pub email: Option<String>,
    #[serde(default = "default_role")]
    pub role: String,
    pub department: Option<String>,
    pub position: Option<String>,
}

fn default_role() -> String {
    "admin".to_string()
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
    pub role: String,
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
    pub role: String,
    /// Plaintext test API key (shown once).
    pub test_api_key: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ResetLinkResponse {
    pub token: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn hash_password(password: &str) -> Result<String, StatusCode> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

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

async fn emit_audit(
    state: &AppState,
    actor: &crate::infrastructure::inbound::http::middleware::jwt_auth::Claims,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    resource_name: &str,
    details: &str,
) {
    if let Some(ref port) = state.audit_port {
        port.record(AuditEvent {
            event_time: Utc::now(),
            account_id: actor.sub,
            account_name: actor.sub.to_string(),
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

// ── GET /v1/accounts ──────────────────────────────────────────────────────────

pub async fn list_accounts(
    RequireSuper(_claims): RequireSuper,
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountSummary>>, StatusCode> {
    let accounts = state
        .account_repo
        .list_all()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(accounts.into_iter().map(to_summary).collect()))
}

// ── POST /v1/accounts ─────────────────────────────────────────────────────────

pub async fn create_account(
    RequireSuper(claims): RequireSuper,
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<CreateAccountResponse>, StatusCode> {
    if req.role != "super" && req.role != "admin" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let password_hash = hash_password(&req.password)?;
    let now = Utc::now();
    let account = Account {
        id: Uuid::now_v7(),
        username: req.username.clone(),
        password_hash,
        name: req.name.clone(),
        email: req.email.clone(),
        role: req.role.clone(),
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
        key_type: "test".to_string(),
        tier: "paid".to_string(),
    };

    // Create account in DB
    state
        .account_repo
        .create(&account)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
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
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut account = state
        .account_repo
        .get_by_id(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(name) = req.name {
        account.name = name;
    }
    account.email = req.email.or(account.email);
    account.department = req.department.or(account.department);
    account.position = req.position.or(account.position);

    state
        .account_repo
        .update(&account)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let account = state
        .account_repo
        .get_by_id(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    state
        .account_repo
        .soft_delete(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    emit_audit(&state, &claims, "delete", "account", &id, &account.username,
        &format!("Account '{}' ({}) soft-deleted (login disabled, data retained)",
            account.username, id)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── PATCH /v1/accounts/{id}/active ────────────────────────────────────────────

pub async fn set_account_active(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<SetActiveRequest>,
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    state
        .account_repo
        .set_active(&uuid, req.is_active)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
) -> Result<Json<Vec<SessionSummary>>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let sessions = state
        .session_repo
        .list_active(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&session_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    state
        .session_repo
        .revoke(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    emit_audit(&state, &claims, "delete", "session", &session_id, &session_id,
        &format!("Session {} manually revoked by admin", session_id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/accounts/{id}/sessions ─────────────────────────────────────────

pub async fn revoke_all_account_sessions(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    state
        .session_repo
        .revoke_all_for_account(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    emit_audit(&state, &claims, "delete", "session", &id, &format!("all_sessions:{id}"),
        &format!("All active sessions for account {} force-revoked by admin", id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── POST /v1/accounts/{id}/reset-link ─────────────────────────────────────────

pub async fn create_reset_link(
    RequireSuper(claims): RequireSuper,
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ResetLinkResponse>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Ensure account exists
    let account = state
        .account_repo
        .get_by_id(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let token = Uuid::new_v4().to_string();

    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = format!("veronex:pwreset:{token}");
        let _: Result<(), _> = pool
            .set(key, uuid.to_string(), Some(fred::types::Expiration::EX(24 * 3600)), None, false)
            .await;
    }

    emit_audit(&state, &claims, "reset_password", "account", &id, &account.username,
        &format!("Password reset link generated for account '{}' ({}); token valid 24h",
            account.username, id)).await;

    Ok(Json(ResetLinkResponse { token }))
}
