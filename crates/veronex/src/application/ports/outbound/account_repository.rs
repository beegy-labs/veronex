use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::Account;

#[async_trait]
pub trait AccountRepository: Send + Sync {
    async fn create(&self, account: &Account) -> Result<()>;
    async fn get_by_id(&self, id: &Uuid) -> Result<Option<Account>>;
    async fn get_by_username(&self, username: &str) -> Result<Option<Account>>;
    async fn list_all(&self) -> Result<Vec<Account>>;
    async fn update(&self, account: &Account) -> Result<()>;
    async fn soft_delete(&self, id: &Uuid) -> Result<()>;
    /// Soft-delete an account and all its API keys in a single transaction.
    /// Returns the number of API keys affected.
    async fn soft_delete_cascade(&self, account_id: &Uuid, tenant_id: &str) -> Result<u64>;
    async fn set_active(&self, id: &Uuid, is_active: bool) -> Result<()>;
    async fn update_last_login(&self, id: &Uuid) -> Result<()>;
    async fn set_password_hash(&self, id: &Uuid, hash: &str) -> Result<()>;
}
