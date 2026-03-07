use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::Session;

#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Insert a new session record.
    async fn create(&self, session: &Session) -> Result<()>;

    /// List all active (non-revoked) sessions for an account.
    async fn list_active(&self, account_id: &Uuid) -> Result<Vec<Session>>;

    /// Look up a session by its refresh token hash.
    async fn get_by_refresh_hash(&self, hash: &str) -> Result<Option<Session>>;

    /// Look up a session by its primary key.
    async fn get_by_id(&self, session_id: &Uuid) -> Result<Option<Session>>;

    /// Revoke a session: sets `revoked_at = now()` in the DB.
    async fn revoke(&self, session_id: &Uuid) -> Result<()>;

    /// Revoke all active sessions for an account.
    async fn revoke_all_for_account(&self, account_id: &Uuid) -> Result<()>;

    /// Non-blocking update of `last_used_at` for the given `jti`.
    async fn update_last_used(&self, jti: &Uuid) -> Result<()>;
}
