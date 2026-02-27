use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::ApiKey;

/// Outbound port for API key persistence.
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    /// Persist a new API key.
    async fn create(&self, key: &ApiKey) -> Result<()>;

    /// Look up a key by its BLAKE2b hash.
    async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>>;

    /// List all keys belonging to a tenant.
    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<ApiKey>>;

    /// Revoke (soft-delete) a key by setting is_active = false.
    async fn revoke(&self, key_id: &Uuid) -> Result<()>;

    /// Toggle is_active without hiding the key.
    async fn set_active(&self, key_id: &Uuid, active: bool) -> Result<()>;

    /// Soft-delete: set deleted_at so the key disappears from list and cannot authenticate.
    async fn soft_delete(&self, key_id: &Uuid) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct MockApiKeyRepository {
        keys: Mutex<Vec<ApiKey>>,
    }

    impl MockApiKeyRepository {
        fn new() -> Self {
            Self {
                keys: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ApiKeyRepository for MockApiKeyRepository {
        async fn create(&self, key: &ApiKey) -> Result<()> {
            self.keys.lock().await.push(key.clone());
            Ok(())
        }

        async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
            let keys = self.keys.lock().await;
            Ok(keys
                .iter()
                .find(|k| k.key_hash == key_hash && k.deleted_at.is_none())
                .cloned())
        }

        async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<ApiKey>> {
            let keys = self.keys.lock().await;
            Ok(keys
                .iter()
                .filter(|k| k.tenant_id == tenant_id && k.deleted_at.is_none())
                .cloned()
                .collect())
        }

        async fn revoke(&self, key_id: &Uuid) -> Result<()> {
            let mut keys = self.keys.lock().await;
            if let Some(key) = keys.iter_mut().find(|k| k.id == *key_id) {
                key.is_active = false;
            }
            Ok(())
        }

        async fn set_active(&self, key_id: &Uuid, active: bool) -> Result<()> {
            let mut keys = self.keys.lock().await;
            if let Some(key) = keys.iter_mut().find(|k| k.id == *key_id) {
                key.is_active = active;
            }
            Ok(())
        }

        async fn soft_delete(&self, key_id: &Uuid) -> Result<()> {
            let mut keys = self.keys.lock().await;
            if let Some(key) = keys.iter_mut().find(|k| k.id == *key_id) {
                key.deleted_at = Some(chrono::Utc::now());
            }
            Ok(())
        }
    }

    fn make_api_key(tenant_id: &str) -> ApiKey {
        ApiKey {
            id: Uuid::now_v7(),
            key_hash: format!("{:064x}", Uuid::now_v7().as_u128()),
            key_prefix: "iq_01ARZ3NDEK".to_string(),
            tenant_id: tenant_id.to_string(),
            name: "test-key".to_string(),
            is_active: true,
            rate_limit_rpm: 0,
            rate_limit_tpm: 0,
            expires_at: None,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn trait_object_creation() {
        let repo: Arc<dyn ApiKeyRepository> = Arc::new(MockApiKeyRepository::new());
        let keys = repo.list_by_tenant("tenant-1").await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn create_and_get_by_hash() {
        let repo = MockApiKeyRepository::new();
        let key = make_api_key("tenant-1");
        let hash = key.key_hash.clone();

        repo.create(&key).await.unwrap();

        let found = repo.get_by_hash(&hash).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, key.id);
        assert_eq!(found.tenant_id, "tenant-1");
    }

    #[tokio::test]
    async fn get_by_hash_returns_none_for_unknown() {
        let repo = MockApiKeyRepository::new();
        let found = repo.get_by_hash("nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn list_by_tenant_filters_correctly() {
        let repo = MockApiKeyRepository::new();
        let key1 = make_api_key("tenant-1");
        let key2 = make_api_key("tenant-1");
        let key3 = make_api_key("tenant-2");

        repo.create(&key1).await.unwrap();
        repo.create(&key2).await.unwrap();
        repo.create(&key3).await.unwrap();

        let t1_keys = repo.list_by_tenant("tenant-1").await.unwrap();
        assert_eq!(t1_keys.len(), 2);

        let t2_keys = repo.list_by_tenant("tenant-2").await.unwrap();
        assert_eq!(t2_keys.len(), 1);

        let t3_keys = repo.list_by_tenant("tenant-3").await.unwrap();
        assert!(t3_keys.is_empty());
    }

    #[tokio::test]
    async fn revoke_sets_inactive() {
        let repo = MockApiKeyRepository::new();
        let key = make_api_key("tenant-1");
        let id = key.id;
        let hash = key.key_hash.clone();

        repo.create(&key).await.unwrap();
        repo.revoke(&id).await.unwrap();

        let found = repo.get_by_hash(&hash).await.unwrap().unwrap();
        assert!(!found.is_active);
    }

    #[tokio::test]
    async fn revoke_nonexistent_is_noop() {
        let repo = MockApiKeyRepository::new();
        let unknown_id = Uuid::now_v7();
        repo.revoke(&unknown_id).await.unwrap();
    }
}
