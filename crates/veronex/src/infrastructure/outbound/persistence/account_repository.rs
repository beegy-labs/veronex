use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::account_repository::AccountRepository;
use crate::domain::entities::Account;

pub struct PostgresAccountRepository {
    pool: PgPool,
}

impl PostgresAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_account(row: &sqlx::postgres::PgRow) -> Result<Account> {
    use sqlx::Row as _;

    Ok(Account {
        id: row.try_get("id").context("id")?,
        username: row.try_get("username").context("username")?,
        password_hash: row.try_get("password_hash").context("password_hash")?,
        name: row.try_get("name").context("name")?,
        email: row.try_get("email").context("email")?,
        role: row.try_get("role").context("role")?,
        department: row.try_get("department").context("department")?,
        position: row.try_get("position").context("position")?,
        is_active: row.try_get("is_active").context("is_active")?,
        created_by: row.try_get("created_by").context("created_by")?,
        last_login_at: row.try_get::<Option<DateTime<Utc>>, _>("last_login_at").context("last_login_at")?,
        created_at: row.try_get("created_at").context("created_at")?,
        deleted_at: row.try_get::<Option<DateTime<Utc>>, _>("deleted_at").context("deleted_at")?,
    })
}

#[async_trait]
impl AccountRepository for PostgresAccountRepository {
    async fn create(&self, account: &Account) -> Result<()> {
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
        .bind(&account.role)
        .bind(&account.department)
        .bind(&account.position)
        .bind(account.is_active)
        .bind(account.created_by)
        .bind(account.created_at)
        .execute(&self.pool)
        .await
        .context("failed to create account")?;

        Ok(())
    }

    async fn get_by_id(&self, id: &Uuid) -> Result<Option<Account>> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, name, email, role, department, position,
                    is_active, created_by, last_login_at, created_at, deleted_at
             FROM accounts
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get account by id")?;

        match row {
            Some(r) => Ok(Some(row_to_account(&r)?)),
            None => Ok(None),
        }
    }

    async fn get_by_username(&self, username: &str) -> Result<Option<Account>> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, name, email, role, department, position,
                    is_active, created_by, last_login_at, created_at, deleted_at
             FROM accounts
             WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get account by username")?;

        match row {
            Some(r) => Ok(Some(row_to_account(&r)?)),
            None => Ok(None),
        }
    }

    async fn list_all(&self) -> Result<Vec<Account>> {
        let rows = sqlx::query(
            "SELECT id, username, password_hash, name, email, role, department, position,
                    is_active, created_by, last_login_at, created_at, deleted_at
             FROM accounts
             WHERE deleted_at IS NULL
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list accounts")?;

        rows.iter().map(|r| row_to_account(r)).collect()
    }

    async fn update(&self, account: &Account) -> Result<()> {
        sqlx::query(
            "UPDATE accounts
             SET name = $2, email = $3, department = $4, position = $5
             WHERE id = $1",
        )
        .bind(account.id)
        .bind(&account.name)
        .bind(&account.email)
        .bind(&account.department)
        .bind(&account.position)
        .execute(&self.pool)
        .await
        .context("failed to update account")?;

        Ok(())
    }

    async fn soft_delete(&self, id: &Uuid) -> Result<()> {
        sqlx::query("UPDATE accounts SET deleted_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to soft-delete account")?;

        Ok(())
    }

    async fn set_active(&self, id: &Uuid, is_active: bool) -> Result<()> {
        sqlx::query("UPDATE accounts SET is_active = $2 WHERE id = $1")
            .bind(id)
            .bind(is_active)
            .execute(&self.pool)
            .await
            .context("failed to set account active state")?;

        Ok(())
    }

    async fn update_last_login(&self, id: &Uuid) -> Result<()> {
        sqlx::query("UPDATE accounts SET last_login_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to update last_login_at")?;

        Ok(())
    }

    async fn set_password_hash(&self, id: &Uuid, hash: &str) -> Result<()> {
        sqlx::query("UPDATE accounts SET password_hash = $2 WHERE id = $1")
            .bind(id)
            .bind(hash)
            .execute(&self.pool)
            .await
            .context("failed to set password hash")?;

        Ok(())
    }
}
