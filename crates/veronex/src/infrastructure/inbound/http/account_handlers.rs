use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::{Account, ApiKey};
use crate::domain::enums::{KeyTier, KeyType};
use crate::domain::services::api_key_generator::generate_api_key;
use crate::domain::services::encryption;
use crate::domain::value_objects::{AccountId, RoleId, SessionId};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireAccountManage;
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
    /// Role IDs to assign (base62 format, e.g. "role_3X4aB..."). Defaults to "viewer".
    #[serde(default)]
    pub role_ids: Vec<RoleId>,
    /// Legacy single role_id — used as fallback when role_ids is empty.
    pub role_id: Option<RoleId>,
    pub department: Option<String>,
    pub position: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateAccountRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub department: Option<String>,
    pub position: Option<String>,
    /// When provided, replaces all role assignments (base62 format).
    pub role_ids: Option<Vec<RoleId>>,
}

#[derive(Deserialize)]
pub struct SetActiveRequest {
    pub is_active: bool,
}

#[derive(Serialize)]
pub struct RoleInfo {
    pub id: RoleId,
    pub name: String,
}

#[derive(Serialize)]
pub struct AccountSummary {
    pub id: AccountId,
    pub username: String,
    pub name: String,
    pub email: Option<String>,
    pub roles: Vec<RoleInfo>,
    pub role_name: String,
    pub permissions: Vec<String>,
    pub menus: Vec<String>,
    pub department: Option<String>,
    pub position: Option<String>,
    pub is_active: bool,
    pub last_login_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct CreateAccountResponse {
    pub id: AccountId,
    pub username: String,
    /// Plaintext test API key (shown once).
    pub test_api_key: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ResetLinkResponse {
    pub token: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

async fn to_summary(a: Account, pg: &sqlx::PgPool) -> Result<AccountSummary, AppError> {
    let role_rows = sqlx::query_as::<_, (Uuid, String, Vec<String>, Vec<String>, bool)>(
        "SELECT r.id, r.name, r.permissions, r.menus, r.is_system
         FROM roles r
         JOIN account_roles ar ON ar.role_id = r.id
         WHERE ar.account_id = $1
         LIMIT 50"
    )
    .bind(a.id)
    .fetch_all(pg)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("role lookup: {e}")))?;

    let mut all_perms = std::collections::BTreeSet::new();
    let mut all_menus = std::collections::BTreeSet::new();
    let mut is_super = false;
    let mut roles = Vec::new();

    for (id, name, perms, menus, is_system) in &role_rows {
        if *is_system && name == "super" { is_super = true; }
        roles.push(RoleInfo { id: RoleId::from_uuid(*id), name: name.clone() });
        for p in perms { all_perms.insert(p.clone()); }
        for m in menus { all_menus.insert(m.clone()); }
    }

    if is_super {
        all_perms = crate::domain::enums::ALL_PERMISSIONS.iter().map(|s| s.to_string()).collect();
        all_menus = crate::domain::enums::ALL_MENUS.iter().map(|s| s.to_string()).collect();
    }

    let role_name = if is_super {
        "super".to_string()
    } else {
        roles.first().map(|r| r.name.clone()).unwrap_or_default()
    };

    Ok(AccountSummary {
        id: AccountId::from_uuid(a.id),
        username: a.username,
        name: a.name,
        email: a.email,
        roles,
        role_name,
        permissions: all_perms.into_iter().collect(),
        menus: all_menus.into_iter().collect(),
        department: a.department,
        position: a.position,
        is_active: a.is_active,
        last_login_at: a.last_login_at,
        created_at: a.created_at,
    })
}

// ── GET /v1/accounts ──────────────────────────────────────────────────────────

use super::handlers::ListPageParams;

pub async fn list_accounts(
    RequireAccountManage(_claims): RequireAccountManage,
    State(state): State<AppState>,
    Query(params): Query<ListPageParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    let page = params.page.unwrap_or(1).clamp(1, super::constants::MAX_PAGE);
    let offset = (page - 1) * limit;

    let (accounts, total) = state
        .account_repo
        .list_page(&search, limit, offset)
        .await?;

    let mut result = Vec::with_capacity(accounts.len());
    for a in accounts {
        result.push(to_summary(a, &state.pg_pool).await?);
    }

    Ok(Json(serde_json::json!({
        "accounts": result,
        "total": total,
        "page": page,
        "limit": limit,
    })))
}

// ── POST /v1/accounts ─────────────────────────────────────────────────────────

pub async fn create_account(
    RequireAccountManage(claims): RequireAccountManage,
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<impl IntoResponse, AppError> {
    super::handlers::validate_username(&req.username)?;

    // Resolve role_ids: explicit role_ids > legacy role_id > default "viewer"
    let role_ids: Vec<Uuid> = if !req.role_ids.is_empty() {
        // Batch-validate all role_ids in a single ANY($1) query — no N+1.
        let input_ids: Vec<Uuid> = req.role_ids.iter().map(|r| r.0).collect();
        let valid: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE id = ANY($1::uuid[]) LIMIT 200",
        )
        .bind(&input_ids as &[Uuid])
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("role check: {e}")))?;
        if valid.len() != input_ids.len() {
            return Err(AppError::BadRequest("one or more invalid role_ids".into()));
        }
        input_ids
    } else if let Some(rid) = req.role_id {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM roles WHERE id = $1)")
            .bind(rid.0)
            .fetch_one(&state.pg_pool)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("role check: {e}")))?;
        if !exists {
            return Err(AppError::BadRequest("invalid role_id".into()));
        }
        vec![rid.0]
    } else {
        // Default to "viewer" role
        let row: (Uuid,) = sqlx::query_as("SELECT id FROM roles WHERE name = 'viewer'")
            .fetch_one(&state.pg_pool)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("viewer role not found: {e}")))?;
        vec![row.0]
    };

    let password_hash = encryption::hash_password(&req.password)?;
    let now = Utc::now();
    let account = Account {
        id: Uuid::now_v7(),
        username: req.username.clone(),
        password_hash,
        name: req.name.clone(),
        email: req.email.clone(),
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
        mcp_cap_points: 3,
        account_id: Some(claims.sub),
    };

    // Create account with roles in a single transaction
    state
        .account_repo
        .create_with_roles(&account, &role_ids)
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
    if let Err(e) = state.api_key_repo.create(&test_key).await {
        tracing::warn!(error = %e, "create_account: failed to create test key");
    }

    emit_audit(&state, &claims, "create", "account", &account.id.to_string(), &req.username,
        &format!("Account '{}' created with {} role(s) and auto-generated test API key",
            req.username, role_ids.len())).await;

    Ok((StatusCode::CREATED, Json(CreateAccountResponse {
        id: AccountId::from_uuid(account.id),
        username: req.username,
        test_api_key: plaintext,
        created_at: now,
    })))
}

// ── PATCH /v1/accounts/{id} ───────────────────────────────────────────────────

pub async fn update_account(
    RequireAccountManage(claims): RequireAccountManage,
    Path(aid): Path<AccountId>,
    State(state): State<AppState>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<StatusCode, AppError> {
    let mut account = state
        .account_repo
        .get_by_id(&aid.0)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("account {aid} not found")))?;

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

    // Update role assignments if provided
    if let Some(role_ids) = &req.role_ids {
        if role_ids.is_empty() {
            return Err(AppError::BadRequest("at least one role is required".into()));
        }
        // Batch-validate all role_ids in a single ANY($1) query — no N+1.
        let input_ids: Vec<Uuid> = role_ids.iter().map(|r| r.0).collect();
        let valid: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE id = ANY($1::uuid[]) LIMIT 200",
        )
        .bind(&input_ids as &[Uuid])
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("role check: {e}")))?;
        if valid.len() != input_ids.len() {
            return Err(AppError::BadRequest("one or more invalid role_ids".into()));
        }
        let uuid_role_ids = input_ids;
        state.account_repo.set_roles(&aid.0, &uuid_role_ids).await?;
    }

    emit_audit(&state, &claims, "update", "account", &aid.to_string(), &account.username,
        &format!("Account '{}' ({}) updated", account.username, aid)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/accounts/{id} ──────────────────────────────────────────────────

pub async fn delete_account(
    RequireAccountManage(claims): RequireAccountManage,
    Path(aid): Path<AccountId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let account = state
        .account_repo
        .get_by_id(&aid.0)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("account {aid} not found")))?;

    // Cascade: soft-delete account + all its API keys in a single transaction
    let keys_deleted = state
        .account_repo
        .soft_delete_cascade(&aid.0, &account.username)
        .await?;

    emit_audit(&state, &claims, "delete", "account", &aid.to_string(), &account.username,
        &format!("Account '{}' ({}) soft-deleted (login disabled, data retained, {} API key(s) revoked)",
            account.username, aid, keys_deleted)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── PATCH /v1/accounts/{id}/active ────────────────────────────────────────────

pub async fn set_account_active(
    RequireAccountManage(claims): RequireAccountManage,
    Path(aid): Path<AccountId>,
    State(state): State<AppState>,
    Json(req): Json<SetActiveRequest>,
) -> Result<StatusCode, AppError> {
    state
        .account_repo
        .set_active(&aid.0, req.is_active)
        .await?;

    emit_audit(&state, &claims, "update", "account", &aid.to_string(), &aid.to_string(),
        &format!("Account {} is_active set to {} (login {})",
            aid, req.is_active, if req.is_active { "enabled" } else { "disabled" })).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── GET /v1/accounts/{id}/sessions ────────────────────────────────────────────

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub ip_address: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
    pub expires_at: chrono::DateTime<Utc>,
}

pub async fn list_account_sessions(
    RequireAccountManage(_claims): RequireAccountManage,
    Path(aid): Path<AccountId>,
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummary>>, AppError> {
    let sessions = state
        .session_repo
        .list_active(&aid.0)
        .await?;

    let summaries = sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: SessionId::from_uuid(s.id),
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
    RequireAccountManage(claims): RequireAccountManage,
    Path(sid): Path<SessionId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    // Fetch the session first to get jti + expires_at for Valkey blocklist.
    let session = state
        .session_repo
        .get_by_id(&sid.0)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("session {sid} not found")))?;
    state.session_repo.revoke(&sid.0).await?;
    // Add JTI to Valkey blocklist so the JWT is rejected immediately.
    super::auth_handlers::revoke_jti(&state, session.jti, session.expires_at).await;
    emit_audit(&state, &claims, "delete", "session", &sid.to_string(), &sid.to_string(),
        &format!("Session {} manually revoked by admin", sid)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/accounts/{id}/sessions ─────────────────────────────────────────

pub async fn revoke_all_account_sessions(
    RequireAccountManage(claims): RequireAccountManage,
    Path(aid): Path<AccountId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    // Fetch active sessions before revoking so we can blocklist each JTI.
    let sessions = state
        .session_repo
        .list_active(&aid.0)
        .await?;
    state.session_repo.revoke_all_for_account(&aid.0).await?;
    // Add each JTI to the Valkey blocklist so JWTs are rejected immediately.
    for session in &sessions {
        super::auth_handlers::revoke_jti(&state, session.jti, session.expires_at).await;
    }
    emit_audit(&state, &claims, "delete", "session", &aid.to_string(), &format!("all_sessions:{aid}"),
        &format!("All active sessions for account {} force-revoked by admin ({} session(s) blocklisted)", aid, sessions.len())).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── POST /v1/accounts/{id}/reset-link ─────────────────────────────────────────

pub async fn create_reset_link(
    RequireAccountManage(claims): RequireAccountManage,
    Path(aid): Path<AccountId>,
    State(state): State<AppState>,
) -> Result<Json<ResetLinkResponse>, AppError> {
    // Ensure account exists
    let account = state
        .account_repo
        .get_by_id(&aid.0)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("account {aid} not found")))?;

    let token = Uuid::new_v4().to_string();

    if let Some(ref pool) = state.valkey_pool {
        use fred::prelude::*;
        let key = valkey_keys::password_reset(&token);
        pool.set(key, aid.0.to_string(), Some(fred::types::Expiration::EX(24 * 3600)), None, false)
            .await
            .unwrap_or_else(|e| tracing::warn!(error = %e, account_id = %aid, "create_reset_link: Valkey SET failed"));
    }

    emit_audit(&state, &claims, "reset_password", "account", &aid.to_string(), &account.username,
        &format!("Password reset link generated for account '{}' ({}); token valid 24h",
            account.username, aid)).await;

    Ok(Json(ResetLinkResponse { token }))
}
