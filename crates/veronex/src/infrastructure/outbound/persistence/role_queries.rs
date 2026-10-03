//! SSOT for `roles` and `account_roles` queries used by role admin
//! handlers.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RoleRow {
    pub id: Uuid,
    pub name: String,
    pub permissions: Vec<String>,
    pub is_system: bool,
    pub created_at: DateTime<Utc>,
}

/// `(name, is_system)` for the role guard.
pub async fn get_identity(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<(String, bool)>> {
    sqlx::query_as::<_, (String, bool)>("SELECT name, is_system FROM roles WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// All roles in registration order, capped to `limit`.
pub async fn list(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<RoleRow>> {
    sqlx::query_as::<_, RoleRow>(
        "SELECT id, name, permissions, is_system, created_at FROM roles \
         ORDER BY created_at ASC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// `(role_id, account_count)` for every role in `role_ids`. Roles with
/// zero active accounts are absent — the caller should default to 0.
pub async fn account_counts(
    pool: &PgPool,
    role_ids: &[Uuid],
) -> sqlx::Result<Vec<(Uuid, i64)>> {
    sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT ar.role_id, COUNT(*)::bigint FROM account_roles ar \
         JOIN accounts a ON a.id = ar.account_id \
         WHERE a.deleted_at IS NULL AND ar.role_id = ANY($1) \
         GROUP BY ar.role_id",
    )
    .bind(role_ids)
    .fetch_all(pool)
    .await
}

/// Total active accounts that still reference `role_id` — used to block
/// delete when assignments remain.
pub async fn count_accounts_for_role(pool: &PgPool, role_id: Uuid) -> sqlx::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM account_roles ar \
         JOIN accounts a ON a.id = ar.account_id \
         WHERE ar.role_id = $1 AND a.deleted_at IS NULL",
    )
    .bind(role_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    permissions: &[String],
    created_at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO roles (id, name, permissions, is_system, created_at) \
         VALUES ($1, $2, $3, FALSE, $4)",
    )
    .bind(id)
    .bind(name)
    .bind(permissions)
    .bind(created_at)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Patch name + permissions, COALESCE-style — `None` keeps the existing value.
pub async fn update_partial(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    permissions: Option<&[String]>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE roles \
         SET name        = COALESCE($2, name), \
             permissions = COALESCE($3, permissions) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(permissions)
    .execute(pool)
    .await
    .map(|_| ())
}

pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM roles WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
}

// ── Role-id existence checks (used by account create/update) ──────────

/// Returns the subset of `candidate_ids` that exist in `roles`. Caller
/// compares lengths to detect invalid ids.
pub async fn ids_in_set(pool: &PgPool, candidate_ids: &[Uuid]) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM roles WHERE id = ANY($1::uuid[]) LIMIT 200")
        .bind(candidate_ids)
        .fetch_all(pool)
        .await
}

pub async fn exists(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM roles WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
}

/// Look up a role id by its (unique) name — used to find the seeded
/// `viewer` default.
pub async fn get_id_by_name(pool: &PgPool, name: &str) -> sqlx::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM roles WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id,)| id))
}

/// `(id, name, permissions, is_system)` for every role assigned to the
/// account. Used by the account-summary builder.
pub async fn list_assignments_for_account(
    pool: &PgPool,
    account_id: Uuid,
) -> sqlx::Result<Vec<(Uuid, String, Vec<String>, bool)>> {
    sqlx::query_as::<_, (Uuid, String, Vec<String>, bool)>(
        "SELECT r.id, r.name, r.permissions, r.is_system \
         FROM roles r \
         JOIN account_roles ar ON ar.role_id = r.id \
         WHERE ar.account_id = $1 \
         LIMIT 50",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
}
