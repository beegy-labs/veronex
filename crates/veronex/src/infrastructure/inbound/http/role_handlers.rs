use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::enums::{ALL_MENUS, ALL_PERMISSIONS};
use crate::domain::value_objects::RoleId;
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireRoleManage;
use crate::infrastructure::inbound::http::state::AppState;

use super::audit_helpers::emit_audit;
use super::error::AppError;

const MAX_ROLES: i64 = 200;

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct RoleSummary {
    pub id: RoleId,
    pub name: String,
    pub permissions: Vec<String>,
    pub menus: Vec<String>,
    pub is_system: bool,
    pub account_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub permissions: Vec<String>,
    pub menus: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
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

fn validate_menus(menus: &[String]) -> Result<(), AppError> {
    for m in menus {
        if !ALL_MENUS.contains(&m.as_str()) {
            return Err(AppError::BadRequest(format!("invalid menu: {m}")));
        }
    }
    Ok(())
}

// ── GET /v1/roles ───────────────────────────────────────────────────────────

pub async fn list_roles(
    RequireRoleManage(_claims): RequireRoleManage,
    State(state): State<AppState>,
) -> Result<Json<Vec<RoleSummary>>, AppError> {
    let rows = sqlx::query_as::<_, (Uuid, String, Vec<String>, Vec<String>, bool, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, name, permissions, menus, is_system, created_at FROM roles ORDER BY created_at ASC LIMIT $1"
    )
    .bind(MAX_ROLES)
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("list roles: {e}")))?;

    if rows.is_empty() {
        return Ok(Json(vec![]));
    }

    // Single batch COUNT query — avoids N round-trips.
    let role_ids: Vec<Uuid> = rows.iter().map(|(id, ..)| *id).collect();
    let count_rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT ar.role_id, COUNT(*)::bigint FROM account_roles ar JOIN accounts a ON a.id = ar.account_id WHERE a.deleted_at IS NULL AND ar.role_id = ANY($1) GROUP BY ar.role_id"
    )
    .bind(&role_ids as &[Uuid])
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("count accounts: {e}")))?;

    let count_map: std::collections::HashMap<Uuid, i64> = count_rows.into_iter().collect();

    let result = rows.into_iter().map(|(id, name, permissions, menus, is_system, created_at)| {
        let account_count = count_map.get(&id).copied().unwrap_or(0);
        RoleSummary { id: RoleId::from_uuid(id), name, permissions, menus, is_system, account_count, created_at }
    }).collect();

    Ok(Json(result))
}

// ── POST /v1/roles ──────────────────────────────────────────────────────────

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
    validate_menus(&req.menus)?;

    let id = Uuid::now_v7();
    let now = chrono::Utc::now();

    sqlx::query(
        "INSERT INTO roles (id, name, permissions, menus, is_system, created_at) VALUES ($1, $2, $3, $4, FALSE, $5)"
    )
    .bind(id)
    .bind(&name)
    .bind(&req.permissions)
    .bind(&req.menus)
    .bind(now)
    .execute(&state.pg_pool)
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
        id: RoleId::from_uuid(id), name, permissions: req.permissions, menus: req.menus,
        is_system: false, account_count: 0, created_at: now,
    })))
}

// ── PATCH /v1/roles/{id} ────────────────────────────────────────────────────

pub async fn update_role(
    RequireRoleManage(claims): RequireRoleManage,
    Path(rid): Path<RoleId>,
    State(state): State<AppState>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<StatusCode, AppError> {
    let row = sqlx::query_as::<_, (String, bool)>(
        "SELECT name, is_system FROM roles WHERE id = $1"
    )
    .bind(rid.0)
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("get role: {e}")))?
    .ok_or_else(|| AppError::NotFound(format!("role {rid} not found")))?;

    if row.1 {
        return Err(AppError::Forbidden("system roles cannot be modified".into()));
    }

    if let Some(ref perms) = req.permissions {
        validate_permissions(perms)?;
    }
    if let Some(ref menus) = req.menus {
        validate_menus(menus)?;
    }

    if req.name.is_none() && req.permissions.is_none() && req.menus.is_none() {
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

    sqlx::query(
        "UPDATE roles \
         SET name        = COALESCE($2, name), \
             permissions = COALESCE($3, permissions), \
             menus       = COALESCE($4, menus) \
         WHERE id = $1",
    )
    .bind(rid.0)
    .bind(name.as_deref())
    .bind(req.permissions.as_deref())
    .bind(req.menus.as_deref())
    .execute(&state.pg_pool)
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

pub async fn delete_role(
    RequireRoleManage(claims): RequireRoleManage,
    Path(rid): Path<RoleId>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let row = sqlx::query_as::<_, (String, bool)>(
        "SELECT name, is_system FROM roles WHERE id = $1"
    )
    .bind(rid.0)
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("get role: {e}")))?
    .ok_or_else(|| AppError::NotFound(format!("role {rid} not found")))?;

    if row.1 {
        return Err(AppError::Forbidden("system roles cannot be deleted".into()));
    }

    let count: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM account_roles ar JOIN accounts a ON a.id = ar.account_id WHERE ar.role_id = $1 AND a.deleted_at IS NULL"
    )
        .bind(rid.0)
        .fetch_one(&state.pg_pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count accounts: {e}")))?;

    if count.0 > 0 {
        return Err(AppError::Conflict(format!(
            "cannot delete role '{}': {} account(s) still assigned", row.0, count.0
        )));
    }

    sqlx::query("DELETE FROM roles WHERE id = $1")
        .bind(rid.0)
        .execute(&state.pg_pool)
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

    #[test]
    fn validate_permissions_empty_slice_ok() {
        assert!(validate_permissions(&[]).is_ok());
    }

    #[test]
    fn validate_menus_accepts_known() {
        let known = ALL_MENUS[0].to_string();
        assert!(validate_menus(&[known]).is_ok());
    }

    #[test]
    fn validate_menus_rejects_unknown() {
        assert!(validate_menus(&["nonexistent_menu".to_string()]).is_err());
    }
}
