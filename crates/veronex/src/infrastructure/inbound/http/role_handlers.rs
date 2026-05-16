use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tracing::instrument;

use crate::domain::enums::ALL_PERMISSIONS;
use crate::domain::value_objects::RoleId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireRoleManage;
use crate::infrastructure::inbound::http::state::AppState;
use crate::infrastructure::outbound::persistence::role_queries as role_q;

use super::audit_helpers::emit_audit;
use super::error::AppError;

const MAX_ROLES: i64 = 200;

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct RoleSummary {
    pub id: RoleId,
    pub name: String,
    pub permissions: Vec<String>,
    pub is_system: bool,
    pub account_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub permissions: Vec<String>,
    /// Legacy field — accepted but ignored. Menu visibility is derived from
    /// `permissions` on the frontend (see `web/lib/route-permissions.ts`).
    #[serde(default)]
    pub menus: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
    /// Legacy field — accepted but ignored. See `CreateRoleRequest::menus`.
    #[serde(default)]
    pub menus: Option<Vec<String>>,
}

// ── Validation ──────────────────────────────────────────────────────────────

fn validate_permissions(perms: &[String]) -> Result<(), AppError> {
    for p in perms {
        if !ALL_PERMISSIONS.contains(&p.as_str()) {
            return Err(AppError::BadRequest(format!("invalid permission: {p}")));
        }
    }
    Ok(())
}

/// Fetch the (name, is_system) pair for a role id; used by update + delete
/// before mutating, both for the system-role guard and the audit-event name.
async fn fetch_role_identity(
    pool: &sqlx::PgPool,
    rid: &RoleId,
) -> Result<(String, bool), AppError> {
    role_q::get_identity(pool, rid.0)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("get role: {e}")))?
        .ok_or_else(|| AppError::NotFound(format!("role {rid} not found")))
}

// ── GET /v1/roles ───────────────────────────────────────────────────────────

#[instrument(skip_all)]

pub async fn list_roles(
    RequireRoleManage(_claims): RequireRoleManage,
    State(state): State<AppState>,
) -> Result<Json<Vec<RoleSummary>>, AppError> {
    let rows = role_q::list(&state.pg_pool, MAX_ROLES)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("list roles: {e}")))?;

    if rows.is_empty() {
        return Ok(Json(vec![]));
    }

    let role_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let count_rows = role_q::account_counts(&state.pg_pool, &role_ids)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count accounts: {e}")))?;
    let count_map: std::collections::HashMap<Uuid, i64> = count_rows.into_iter().collect();

    let result = rows.into_iter().map(|r| {
        let account_count = count_map.get(&r.id).copied().unwrap_or(0);
        RoleSummary {
            id: RoleId::from_uuid(r.id),
            name: r.name,
            permissions: r.permissions,
            is_system: r.is_system,
            account_count,
            created_at: r.created_at,
        }
    }).collect();

    Ok(Json(result))
}

// ── POST /v1/roles ──────────────────────────────────────────────────────────

#[instrument(skip_all)]

pub async fn create_role(
    RequireRoleManage(claims): RequireRoleManage,
    State(state): State<AppState>,
    Json(req): Json<CreateRoleRequest>,
) -> Result<impl IntoResponse, AppError> {
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 64 {
        return Err(AppError::BadRequest("role name must be 1-64 characters".into()));
    }
    validate_permissions(&req.permissions)?;

    let id = Uuid::now_v7();
    let now = chrono::Utc::now();

    role_q::insert(&state.pg_pool, id, &name, &req.permissions, now)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                AppError::Conflict(format!("role '{}' already exists", name))
            } else {
                AppError::Internal(anyhow::anyhow!("create role: {e}"))
            }
        })?;

    emit_audit(&state, &claims, "create", "role", &id.to_string(), &name,
        &format!("Role '{}' created with permissions: {:?}", name, req.permissions)).await;

    Ok((StatusCode::CREATED, Json(RoleSummary {
        id: RoleId::from_uuid(id), name, permissions: req.permissions,
        is_system: false, account_count: 0, created_at: now,
    })))
}

// ── PATCH /v1/roles/{id} ────────────────────────────────────────────────────

#[instrument(skip_all)]

pub async fn update_role(
    RequireRoleManage(claims): RequireRoleManage,
    Path(rid): Path<RoleId>,
    State(state): State<AppState>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<StatusCode, AppError> {
    let row = fetch_role_identity(&state.pg_pool, &rid).await?;
    if row.1 {
        return Err(AppError::Forbidden("system roles cannot be modified".into()));
    }

    if let Some(ref perms) = req.permissions {
        validate_permissions(perms)?;
    }

    if req.name.is_none() && req.permissions.is_none() {
        return Ok(StatusCode::NO_CONTENT);
    }

    let name = match req.name.as_ref() {
        Some(n) => {
            let trimmed = n.trim().to_string();
            if trimmed.is_empty() || trimmed.len() > 64 {
                return Err(AppError::BadRequest("role name must be 1-64 characters".into()));
            }
            Some(trimmed)
        }
        None => None,
    };

    role_q::update_partial(&state.pg_pool, rid.0, name.as_deref(), req.permissions.as_deref())
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                AppError::Conflict("role name already exists".into())
            } else {
                AppError::Internal(anyhow::anyhow!("update role: {e}"))
            }
        })?;

    emit_audit(&state, &claims, "update", "role", &rid.to_string(), &row.0,
        &format!("Role '{}' updated", row.0)).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/roles/{id} ───────────────────────────────────────────────────

#[instrument(skip_all)]

pub async fn delete_role(
    RequireRoleManage(claims): RequireRoleManage,
    Path(rid): Path<RoleId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let row = fetch_role_identity(&state.pg_pool, &rid).await?;
    if row.1 {
        return Err(AppError::Forbidden("system roles cannot be deleted".into()));
    }

    let count = role_q::count_accounts_for_role(&state.pg_pool, rid.0)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count accounts: {e}")))?;

    if count > 0 {
        return Err(AppError::Conflict(format!(
            "cannot delete role '{}': {} account(s) still assigned", row.0, count
        )));
    }

    role_q::delete(&state.pg_pool, rid.0)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("delete role: {e}")))?;

    emit_audit(&state, &claims, "delete", "role", &rid.to_string(), &row.0,
        &format!("Role '{}' deleted", row.0)).await;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_permissions_accepts_known() {
        let known = ALL_PERMISSIONS[0].to_string();
        assert!(validate_permissions(&[known]).is_ok());
    }

    #[test]
    fn validate_permissions_rejects_unknown() {
        assert!(validate_permissions(&["not_a_real_permission".to_string()]).is_err());
    }

}
