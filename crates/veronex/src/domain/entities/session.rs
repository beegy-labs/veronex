use chrono::{DateTime, Utc};
use uuid::Uuid;

/// An active or revoked login session.
///
/// `jti` is the JWT ID — a unique UUIDv7 included in every access token.
/// When a session is revoked, `jti` is added to the Valkey blocklist so that
/// any in-flight access token using that `jti` is immediately rejected.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: Uuid,
    pub account_id: Uuid,
    /// JWT ID — matches the `jti` claim in the issued access token.
    pub jti: Uuid,
    /// Blake2b-256 hex hash of the refresh token (stored for rolling refresh).
    pub refresh_token_hash: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    /// Access token TTL end (used for Valkey blocklist TTL on revoke).
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}
