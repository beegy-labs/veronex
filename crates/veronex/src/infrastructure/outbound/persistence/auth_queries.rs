//! SSOT for the first-run setup transaction. Per-account role
//! resolution lives in `role_queries::list_assignments_for_account`
//! (the same SQL serves both auth and account handlers).

use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

/// Setup namespace — keeps the magic number out of handler code.
pub const SETUP_ADVISORY_LOCK: i64 = 0xBEE6_0001;

/// Acquire the per-tx advisory lock so two concurrent setup attempts
/// cannot both pass the "no accounts exist" guard.
pub async fn acquire_setup_lock(tx: &mut Transaction<'_, Postgres>) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(SETUP_ADVISORY_LOCK)
        .execute(&mut **tx as &mut PgConnection)
        .await
        .map(|_| ())
}

/// Count of live accounts (excluding soft-deletes).
pub async fn count_live_accounts(tx: &mut Transaction<'_, Postgres>) -> sqlx::Result<i64> {
    let row: (i64,) =
        sqlx::query_as("SELECT count(*) FROM accounts WHERE deleted_at IS NULL")
            .fetch_one(&mut **tx as &mut PgConnection)
            .await?;
    Ok(row.0)
}

/// Look up the seeded `super` role id (created by migration 000007).
pub async fn get_super_role_id(tx: &mut Transaction<'_, Postgres>) -> sqlx::Result<Uuid> {
    let row: (Uuid,) = sqlx::query_as("SELECT id FROM roles WHERE name = 'super'")
        .fetch_one(&mut **tx as &mut PgConnection)
        .await?;
    Ok(row.0)
}

/// Insert the first-run super account row. All metadata fields are
/// left null/default — the caller passes a fully-populated `Account`.
#[allow(clippy::too_many_arguments)]
pub async fn insert_setup_account(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    username: &str,
    password_hash: &str,
    name: &str,
    email: Option<&str>,
    department: Option<&str>,
    position: Option<&str>,
    is_active: bool,
    created_by: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO accounts \
         (id, username, password_hash, name, email, department, position, \
          is_active, created_by, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(username)
    .bind(password_hash)
    .bind(name)
    .bind(email)
    .bind(department)
    .bind(position)
    .bind(is_active)
    .bind(created_by)
    .bind(created_at)
    .execute(&mut **tx as &mut PgConnection)
    .await
    .map(|_| ())
}

/// Assign a role via the join table.
pub async fn assign_role(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    role_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO account_roles (account_id, role_id) VALUES ($1, $2)")
        .bind(account_id)
        .bind(role_id)
        .execute(&mut **tx as &mut PgConnection)
        .await
        .map(|_| ())
}
