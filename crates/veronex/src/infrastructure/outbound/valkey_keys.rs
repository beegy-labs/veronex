//! Valkey key registry for all `veronex:*` key patterns.
//!
//! Queue constants (`QUEUE_*`) are defined in `domain::constants` (the SSOT)
//! and re-exported here for infrastructure convenience.  All other keys are
//! constructed through functions in this module.

use uuid::Uuid;

// ── Queue keys (SSOT in domain::constants) ───────────────────────────────────

pub use crate::domain::constants::{QUEUE_JOBS, QUEUE_JOBS_PAID, QUEUE_JOBS_TEST, QUEUE_PROCESSING};

// ── ZSET queue keys (Phase 3 — SSOT in domain::constants) ────────────────────

pub use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP};

/// Demand counter key for a specific model.
pub fn demand_counter(model: &str) -> String {
    crate::domain::constants::demand_key(model)
}

// ── Rate limiting ────────────────────────────────────────────────────────────

/// RPM (requests per minute) sorted-set key for an API key.
pub fn ratelimit_rpm(key_id: Uuid) -> String {
    format!("veronex:ratelimit:rpm:{key_id}")
}

/// TPM (tokens per minute) counter key for an API key at a given minute epoch.
pub fn ratelimit_tpm(key_id: Uuid, minute: i64) -> String {
    crate::domain::constants::ratelimit_tpm_key(key_id, minute)
}

// ── Auth / session ───────────────────────────────────────────────────────────

/// Key for a revoked JWT (stored until natural expiry).
pub fn revoked_jti(jti: Uuid) -> String {
    format!("veronex:revoked:{jti}")
}

/// Key for a password-reset token (24 h TTL).
pub fn password_reset(token: &str) -> String {
    format!("veronex:pwreset:{token}")
}

/// Key for a used refresh token hash (prevents replay attacks).
pub fn refresh_blocklist(hash: &str) -> String {
    format!("veronex:refresh_used:{hash}")
}

/// IP-based login attempt counter (5-minute sliding window).
pub fn login_attempts(ip: &str) -> String {
    format!("veronex:login_attempts:{ip}")
}

// ── Provider infrastructure ──────────────────────────────────────────────────

/// Thermal throttle level cache for a provider.
pub fn thermal_throttle(provider_id: Uuid) -> String {
    format!("veronex:throttle:{provider_id}")
}

/// Cached Ollama model list for a provider.
pub fn provider_models(provider_id: Uuid) -> String {
    format!("veronex:models:{provider_id}")
}

/// Hardware metrics cache for a provider's GPU server.
pub fn hw_metrics(provider_id: Uuid) -> String {
    format!("veronex:hw:{provider_id}")
}

// ── Gemini rate-limit counters ───────────────────────────────────────────────

/// Gemini RPM counter (per provider + model + minute).
pub fn gemini_rpm(provider_id: Uuid, model: &str, minute: i64) -> String {
    format!("veronex:gemini:rpm:{provider_id}:{model}:{minute}")
}

/// Gemini RPD counter (per provider + model + date).
pub fn gemini_rpd(provider_id: Uuid, model: &str, date: &str) -> String {
    format!("veronex:gemini:rpd:{provider_id}:{model}:{date}")
}

// ── Multi-instance coordination ─────────────────────────────────────────────

/// Instance heartbeat key (EX 30s, refreshed every 10s).
pub fn heartbeat(instance_id: &str) -> String {
    format!("veronex:heartbeat:{instance_id}")
}

/// HASH tracking per-instance slot counts for a (provider, model) pair.
/// Fields: `{instance_id}` → count, `__max__` → max slots.
pub fn distributed_slots(provider_id: Uuid, model: &str) -> String {
    format!("veronex:slots:{provider_id}:{model}")
}

/// ZSET of slot leases for crash recovery.
/// Members: `{instance_id}:{lease_id}`, scores: expiry timestamp.
pub fn slot_leases(provider_id: Uuid, model: &str) -> String {
    format!("veronex:slot_leases:{provider_id}:{model}")
}

/// Tracks which instance owns a running job (EX 300s).
pub fn job_owner(job_id: Uuid) -> String {
    crate::domain::constants::job_owner_key(job_id)
}

/// Valkey Stream key for cross-instance token relay (XADD/XREAD).
///
/// Uses Streams instead of Pub/Sub to prevent initial token black hole —
/// late-connecting subscribers can read from `0-0` to catch up.
pub fn stream_tokens(job_id: Uuid) -> String {
    format!("veronex:stream:tokens:{job_id}")
}

/// Pub/sub channel for cross-instance job status events.
pub const PUBSUB_JOB_EVENTS: &str = "veronex:pubsub:job_events";

/// Pub/sub channel for cross-instance cancellation signals.
pub fn pubsub_cancel(job_id: Uuid) -> String {
    format!("veronex:pubsub:cancel:{job_id}")
}

/// Pattern for subscribing to all cancel channels.
pub const PUBSUB_CANCEL_PATTERN: &str = "veronex:pubsub:cancel:*";

// ── Provider liveness (agent heartbeat) ─────────────────────────────────────

/// Heartbeat key set by veronex-agent after each successful Ollama scrape.
/// TTL = 3× scrape interval (default 90s). Missing key = provider offline.
/// Written by: veronex-agent. Read by: health_checker (MGET batch).
pub fn provider_heartbeat(provider_id: Uuid) -> String {
    format!("veronex:provider:hb:{provider_id}")
}

/// Global O(1) counter of currently-online Ollama providers.
/// Incremented/decremented atomically by health_checker on status transitions.
/// Read by dashboard to avoid SELECT COUNT(*) from DB.
pub const PROVIDERS_ONLINE_COUNTER: &str = "veronex:stats:providers:online";

// ── VRAM pool ───────────────────────────────────────────────────────────────

/// Valkey key tracking total reserved VRAM (MB) per provider.
pub fn vram_reserved(provider_id: Uuid) -> String {
    format!("veronex:vram_reserved:{provider_id}")
}

/// ZSET of VRAM lease entries for crash recovery.
pub fn vram_leases(provider_id: Uuid) -> String {
    format!("veronex:vram_leases:{provider_id}")
}

// ── Placement planner (SSOT in domain::constants) ───────────────────────────

pub use crate::domain::constants::{preload_lock_key as preload_lock};
pub use crate::domain::constants::{scaleout_decision_key as scaleout_decision};
