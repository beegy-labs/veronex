use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::session_repository::SessionRepository;
use crate::domain::entities::Session;

/// Column list shared by all SELECT queries on account_sessions.
const SESSION_COLS: &str = "id, account_id, jti, refresh_token_hash, ip_address, \
    created_at, last_used_at, expires_at, revoked_at";

pub struct PostgresSessionRepository {
    pool: PgPool,
}

impl PostgresSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_session(row: &sqlx::postgres::PgRow) -> Result<Session> {
    use sqlx::Row as _;

    Ok(Session {
        id: row.try_get("id").context("id")?,
        account_id: row.try_get("account_id").context("account_id")?,
        jti: row.try_get("jti").context("jti")?,
        refresh_token_hash: row
            .try_get::<Option<String>, _>("refresh_token_hash")
            .context("refresh_token_hash")?,
        ip_address: row
            .try_get::<Option<String>, _>("ip_address")
            .context("ip_address")?,
        created_at: row.try_get("created_at").context("created_at")?,
        last_used_at: row
            .try_get::<Option<DateTime<Utc>>, _>("last_used_at")
            .context("last_used_at")?,
        expires_at: row.try_get("expires_at").context("expires_at")?,
        revoked_at: row
            .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
            .context("revoked_at")?,
    })
}

#[async_trait]
impl SessionRepository for PostgresSessionRepository {
    async fn create(&self, session: &Session) -> Result<()> {
        sqlx::query(
            "INSERT INTO account_sessions
                (id, account_id, jti, refresh_token_hash, ip_address, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(session.id)
        .bind(session.account_id)
        .bind(session.jti)
        .bind(&session.refresh_token_hash)
        .bind(&session.ip_address)
        .bind(session.created_at)
        .bind(session.expires_at)
        .execute(&self.pool)
        .await
        .context("failed to create session")?;
        Ok(())
    }

    async fn list_active(&self, account_id: &Uuid) -> Result<Vec<Session>> {
        let sql = format!("SELECT {SESSION_COLS} FROM account_sessions WHERE account_id = $1 AND revoked_at IS NULL ORDER BY created_at DESC LIMIT 1000");
        let rows = sqlx::query(&sql)
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .context("failed to list active sessions")?;

        rows.iter().map(row_to_session).collect()
    }

    async fn get_by_refresh_hash(&self, hash: &str) -> Result<Option<Session>> {
        let sql = format!("SELECT {SESSION_COLS} FROM account_sessions WHERE refresh_token_hash = $1 AND revoked_at IS NULL");
        let row = sqlx::query(&sql)
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get session by refresh hash")?;

        match row {
            Some(r) => Ok(Some(row_to_session(&r)?)),
            None => Ok(None),
        }
    }

    async fn get_by_id(&self, session_id: &Uuid) -> Result<Option<Session>> {
        let sql = format!("SELECT {SESSION_COLS} FROM account_sessions WHERE id = $1");
        let row = sqlx::query(&sql)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get session by id")?;

        match row {
            Some(r) => Ok(Some(row_to_session(&r)?)),
            None => Ok(None),
        }
    }

    async fn revoke(&self, session_id: &Uuid) -> Result<()> {
        sqlx::query("UPDATE account_sessions SET revoked_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("failed to revoke session")?;
        Ok(())
    }

    async fn revoke_all_for_account(&self, account_id: &Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE account_sessions SET revoked_at = now()
             WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(account_id)
        .execute(&self.pool)
        .await
        .context("failed to revoke all sessions for account")?;
        Ok(())
    }

    async fn update_last_used(&self, jti: &Uuid) -> Result<()> {
        sqlx::query("UPDATE account_sessions SET last_used_at = now() WHERE jti = $1")
            .bind(jti)
            .execute(&self.pool)
            .await
            .context("failed to update last_used_at")?;
        Ok(())
    }
}
