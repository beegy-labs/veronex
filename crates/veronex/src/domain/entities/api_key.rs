use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use super::super::enums::{KeyTier, KeyType};

/// API key entity for tenant authentication.
///
/// The plaintext key is never stored — only the BLAKE2b-256 hash.
/// The `key_prefix` (first 12 chars) is kept for display purposes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct ApiKey {
    pub id: Uuid,
    #[serde(skip_serializing)]
    #[ts(skip)]
    pub key_hash: String,
    pub key_prefix: String,
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,
    pub rate_limit_tpm: i32,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Soft-delete timestamp. When set, key is hidden from list and blocked from auth.
    #[serde(default)]
    #[ts(skip)]
    pub deleted_at: Option<DateTime<Utc>>,
    /// Key category: standard (production) or test (dev/testing).
    #[serde(default)]
    #[ts(skip)]
    pub key_type: KeyType,
    /// Billing tier: free or paid (default).
    #[serde(default)]
    pub tier: KeyTier,
    /// Account that created this key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(skip)]
    pub account_id: Option<Uuid>,
}

/// Returned once at key creation — contains the plaintext key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub key: String,
    pub key_prefix: String,
    pub tenant_id: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_api_key() -> ApiKey {
        ApiKey {
            id: Uuid::now_v7(),
            key_hash: "a".repeat(64),
            key_prefix: "vnx_01ARZ3N".to_string(),
            tenant_id: "tenant-1".to_string(),
            name: "test-key".to_string(),
            is_active: true,
            rate_limit_rpm: 0,
            rate_limit_tpm: 0,
            expires_at: None,
            created_at: Utc::now(),
            deleted_at: None,
            key_type: KeyType::Standard,
            tier: KeyTier::Paid,
            account_id: None,
        }
    }

    #[test]
    fn api_key_creation() {
        let key = make_api_key();
        assert_eq!(key.id.get_version_num(), 7);
        assert_eq!(key.tenant_id, "tenant-1");
        assert_eq!(key.name, "test-key");
        assert!(key.is_active);
        assert_eq!(key.rate_limit_rpm, 0);
        assert_eq!(key.rate_limit_tpm, 0);
        assert!(key.expires_at.is_none());
    }

    #[test]
    fn api_key_with_rate_limits() {
        let mut key = make_api_key();
        key.rate_limit_rpm = 60;
        key.rate_limit_tpm = 100_000;
        assert_eq!(key.rate_limit_rpm, 60);
        assert_eq!(key.rate_limit_tpm, 100_000);
    }

    #[test]
    fn api_key_with_expiry() {
        let mut key = make_api_key();
        let expires = Utc::now() + chrono::Duration::days(30);
        key.expires_at = Some(expires);
        assert!(key.expires_at.is_some());
    }

    #[test]
    fn api_key_serialization_omits_key_hash() {
        let key = make_api_key();
        let json = serde_json::to_string(&key).unwrap();
        // key_hash must not appear in serialized output
        assert!(!json.contains("key_hash"));
        // public fields must be present
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["key_prefix"], "vnx_01ARZ3N");
        assert_eq!(v["tenant_id"], "tenant-1");
        assert_eq!(v["name"], "test-key");
        assert_eq!(v["is_active"], true);
    }

    #[test]
    fn api_key_serde_with_expires_at() {
        let mut key = make_api_key();
        key.expires_at = Some(Utc::now());
        let json = serde_json::to_string(&key).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["expires_at"].is_string());
    }

    #[test]
    fn api_key_created_serde_roundtrip() {
        let created = ApiKeyCreated {
            id: Uuid::now_v7(),
            key: "vnx_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            key_prefix: "vnx_01ARZ3NDE".to_string(),
            tenant_id: "tenant-1".to_string(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&created).unwrap();
        let deserialized: ApiKeyCreated = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, created.id);
        assert_eq!(deserialized.key, created.key);
        assert_eq!(deserialized.key_prefix, created.key_prefix);
        assert_eq!(deserialized.tenant_id, created.tenant_id);
    }

    #[test]
    fn api_key_inactive() {
        let mut key = make_api_key();
        key.is_active = false;
        assert!(!key.is_active);
    }
}
