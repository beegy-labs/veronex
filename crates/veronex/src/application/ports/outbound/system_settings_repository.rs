//! Phase 3 — `system_settings` repository port.
//!
//! Generic key/value bag for runtime-tunable knobs (idle TTLs, AIMD weights,
//! capacity heuristics). Distinct from `app_config`:
//!
//! - `app_config` holds bootstrap-time secrets (S3, HF tokens) that
//!   require a restart and are encrypted at rest.
//! - `system_settings` holds plain TEXT values that take effect on the
//!   next read; consumers cache them with a short TTL.
//!
//! Typed parsing happens in the consuming module — keeping the schema
//! generic means adding a knob never migrates the table.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSetting {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

#[async_trait]
pub trait SystemSettingsRepository: Send + Sync {
    /// Fetch a single setting. `None` when the key has never been set.
    async fn get(&self, key: &str) -> Result<Option<SystemSetting>>;

    /// List every setting (admin UI). Caller filters / sorts.
    async fn list_all(&self) -> Result<Vec<SystemSetting>>;

    /// Upsert. Touches `updated_at`; sets `updated_by` when provided.
    async fn upsert(
        &self,
        key: &str,
        value: &str,
        description: Option<&str>,
        by: Option<Uuid>,
    ) -> Result<()>;

    /// Hard delete. Returns whether a row was removed (admin UI maps
    /// `false` to 404).
    async fn delete(&self, key: &str) -> Result<bool>;
}

// ── Canonical keys ──────────────────────────────────────────────────────────
//
// Every consumer must read settings via these constants — keeps the
// shared key namespace under one roof and lets future migrations grep
// for existing readers.

/// Idle reaper TTL in seconds. `0` disables idle reaping entirely.
pub const KEY_IDLE_TTL_SECONDS: &str = "llama_server.idle_ttl_seconds";

/// Warmup probe `max_tokens` for the short prompt.
pub const KEY_WARMUP_SHORT_TOKENS: &str = "llama_server.warmup_short_tokens";

/// Warmup probe `max_tokens` for the long prompt (75% of `n_ctx`).
pub const KEY_WARMUP_LONG_TOKENS: &str = "llama_server.warmup_long_tokens";

/// Default values applied when a key is absent from the table. Mirrors
/// the seed in `init.sql`; consumers fall back here so a fresh install
/// has sensible behaviour before the operator visits the admin UI.
pub fn default_for(key: &str) -> Option<&'static str> {
    match key {
        KEY_IDLE_TTL_SECONDS => Some("60"),
        KEY_WARMUP_SHORT_TOKENS => Some("8"),
        KEY_WARMUP_LONG_TOKENS => Some("8"),
        _ => None,
    }
}
