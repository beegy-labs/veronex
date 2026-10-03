//! Outbound port — runtime-mutable app config (first-install wizard).
//!
//! Holds the values that an open-source operator supplies through
//! `POST /v1/setup/storage` rather than environment variables: Garage S3
//! endpoint + access keys, HuggingFace token, Local PV path, max disk
//! capacity, etc. Env var still wins when present, so the same binary works
//! in both Kubernetes (env-driven) and self-hosted (DB-driven) modes.
//!
//! Secret values (S3 secret key, HF token) are stored as aes-gcm ciphertext;
//! the repo handles encryption transparently using the same master key as
//! `llm_providers.api_key_encrypted`.
//!
//! # Canonical keys
//!
//! Defined as `&'static str` constants so the wizard handler, the bootstrap
//! reader, and the admin GET endpoint all agree on the spelling. Add new
//! keys here whenever a new setting joins the wizard.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A row in `app_config`. The `value` is the plaintext form — the repo
/// transparently encrypts/decrypts on write/read for `is_secret = true`.
#[derive(Debug, Clone)]
pub struct AppConfigEntry {
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

/// Plain-text setter request bundle. Used by the wizard handler.
#[derive(Debug, Clone)]
pub struct AppConfigUpsert {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

#[async_trait]
pub trait AppConfigRepository: Send + Sync {
    /// Read a single key. Returns the plaintext value (decrypted if secret),
    /// or `None` when the row is absent. Decryption errors propagate.
    async fn get(&self, key: &str) -> Result<Option<AppConfigEntry>>;

    /// Read multiple keys in one round-trip. Missing keys are simply absent
    /// from the result map; callers fall back to env / default.
    async fn get_many(&self, keys: &[&str]) -> Result<Vec<AppConfigEntry>>;

    /// List every key. Used by the admin GET endpoint to render the config
    /// editor; secrets are masked at the handler boundary, not here.
    async fn list_all(&self) -> Result<Vec<AppConfigEntry>>;

    /// UPSERT a single key. The repo encrypts when `entry.is_secret = true`.
    async fn upsert(&self, entry: &AppConfigUpsert, by: Option<Uuid>) -> Result<()>;

    /// Bulk UPSERT. Implementation runs each row in the same transaction so
    /// a partial wizard submit either fully lands or fully rolls back —
    /// avoids "S3 endpoint set but secret key missing" half-states.
    async fn upsert_many(
        &self,
        entries: &[AppConfigUpsert],
        by: Option<Uuid>,
    ) -> Result<()>;

    /// Remove a single key (admin "clear" action).
    async fn delete(&self, key: &str) -> Result<bool>;
}

// ── Canonical config keys ──────────────────────────────────────────────────
//
// Keep these synchronised with the wizard frontend and the bootstrap loader.
// Plain string constants beat an enum here because Postgres stores them
// verbatim and the wizard handler maps free-form JSON to the same names.

/// S3 endpoint URL (Garage / MinIO / AWS). Example: `http://garage.svc:3900`.
pub const KEY_S3_ENDPOINT: &str = "s3.endpoint";
/// S3 region (Garage uses `garage` by default; AWS uses real region names).
pub const KEY_S3_REGION: &str = "s3.region";
/// S3 access key id. Non-secret (it's an identifier).
pub const KEY_S3_ACCESS_KEY: &str = "s3.access_key";
/// S3 secret key. **Secret — encrypted at rest.**
pub const KEY_S3_SECRET_KEY: &str = "s3.secret_key";
/// Bucket name for GGUF blob CAS. Non-secret.
pub const KEY_S3_MODEL_BUCKET: &str = "s3.model_bucket";

/// HuggingFace bearer token. **Secret — encrypted at rest.**
pub const KEY_HF_TOKEN: &str = "hf.token";
/// Optional HF API endpoint override (default `https://huggingface.co`).
pub const KEY_HF_ENDPOINT: &str = "hf.endpoint";

/// Filesystem path for Local PV blob cache. Operator-controlled.
pub const KEY_MODEL_LOCAL_PATH: &str = "model_store.local_path";
/// Maximum disk usage for Local PV cache in GB.
pub const KEY_MODEL_MAX_DISK_GB: &str = "model_store.max_disk_gb";

/// All canonical keys — the wizard uses this to render the form,
/// bootstrap iterates it for env > DB merge.
pub const ALL_KEYS: &[(&str, bool)] = &[
    (KEY_S3_ENDPOINT, false),
    (KEY_S3_REGION, false),
    (KEY_S3_ACCESS_KEY, false),
    (KEY_S3_SECRET_KEY, true),
    (KEY_S3_MODEL_BUCKET, false),
    (KEY_HF_TOKEN, true),
    (KEY_HF_ENDPOINT, false),
    (KEY_MODEL_LOCAL_PATH, false),
    (KEY_MODEL_MAX_DISK_GB, false),
];

/// Helper: is this key marked secret in the canonical list?
pub fn is_secret_key(key: &str) -> bool {
    ALL_KEYS.iter().any(|(k, sec)| *k == key && *sec)
}
