//! pk-aware shims for direct-fred call sites in the infrastructure layer.
//!
//! Canonical Valkey key strings/constructors are the SSOT in
//! [`crate::domain::constants`]. The deployment-time `VALKEY_KEY_PREFIX`
//! (set via `init_prefix(...)` once at startup) is applied here.
//!
//! Application code does **not** use this module — it imports
//! `crate::domain::constants::*_key()` directly and lets `ValkeyAdapter`
//! prepend the prefix transparently when the key crosses the port boundary.
//!
//! Infrastructure code that bypasses `ValkeyPort` and talks to `fred`
//! directly (e.g. mcp/bridge cache invalidation, capacity analyzer
//! lookups, pubsub relay) goes through these shims so the prefix is
//! never accidentally skipped.

use std::sync::OnceLock;
use uuid::Uuid;

use crate::domain::constants as d;

// ── Global key prefix ────────────────────────────────────────────────────────

static KEY_PREFIX: OnceLock<Box<str>> = OnceLock::new();

/// Set the global Valkey key prefix. Must be called once at startup before any
/// Valkey operations. Subsequent calls are silent no-ops.
///
/// `prefix` should end with a delimiter (e.g. `"prod:"`) so resulting keys read
/// as `"prod:veronex:queue:zset"`. An empty string disables prefixing.
pub fn init_prefix(prefix: &str) {
    KEY_PREFIX.set(Box::from(prefix)).ok();
}

/// Apply the global prefix to `key`. Returns `key` unchanged when prefix is empty.
#[inline]
pub fn pk(key: &str) -> String {
    let pfx = p();
    if pfx.is_empty() {
        key.to_string()
    } else {
        format!("{pfx}{key}")
    }
}

#[inline]
fn p() -> &'static str {
    KEY_PREFIX.get().map(|s| s.as_ref()).unwrap_or("")
}

// ── Queue keys (parameterless) ───────────────────────────────────────────────

pub use crate::domain::constants::{
    QUEUE_JOBS, QUEUE_JOBS_PAID, QUEUE_JOBS_TEST, QUEUE_PROCESSING,
    QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP,
};

pub fn queue_jobs() -> String { pk(d::QUEUE_JOBS) }
pub fn queue_jobs_paid() -> String { pk(d::QUEUE_JOBS_PAID) }
pub fn queue_jobs_test() -> String { pk(d::QUEUE_JOBS_TEST) }
pub fn queue_processing() -> String { pk(d::QUEUE_PROCESSING) }
pub fn queue_active() -> String { pk(d::QUEUE_ACTIVE) }
pub fn queue_active_attempts() -> String { pk(d::QUEUE_ACTIVE_ATTEMPTS) }
pub fn queue_zset() -> String { pk(d::QUEUE_ZSET) }
pub fn queue_enqueue_at() -> String { pk(d::QUEUE_ENQUEUE_AT) }
pub fn queue_model_map() -> String { pk(d::QUEUE_MODEL_MAP) }

pub fn demand_counter(model: &str) -> String { pk(&d::demand_key(model)) }

// ── Rate limiting ────────────────────────────────────────────────────────────

pub fn ratelimit_rpm(key_id: Uuid) -> String { pk(&d::ratelimit_rpm_key(key_id)) }
pub fn ratelimit_tpm(key_id: Uuid, minute: i64) -> String { pk(&d::ratelimit_tpm_key(key_id, minute)) }

// ── Auth / session ───────────────────────────────────────────────────────────

pub fn revoked_jti(jti: Uuid) -> String { pk(&d::revoked_jti_key(jti)) }
pub fn password_reset(token: &str) -> String { pk(&d::password_reset_key(token)) }
pub fn refresh_blocklist(hash: &str) -> String { pk(&d::refresh_blocklist_key(hash)) }
pub fn login_attempts(ip: &str) -> String { pk(&d::login_attempts_key(ip)) }

// ── Provider infrastructure ──────────────────────────────────────────────────

pub fn thermal_throttle(provider_id: Uuid) -> String { pk(&d::thermal_throttle_key(provider_id)) }
pub fn provider_models(provider_id: Uuid) -> String { pk(&d::provider_models_key(provider_id)) }
pub fn hw_metrics(provider_id: Uuid) -> String { pk(&d::hw_metrics_key(provider_id)) }
pub fn server_node_metrics(server_id: Uuid) -> String { pk(&d::server_node_metrics_key(server_id)) }

// ── Gemini rate-limit counters ───────────────────────────────────────────────

pub fn gemini_rpm(provider_id: Uuid, model: &str, minute: i64) -> String {
    pk(&d::gemini_rpm_key(provider_id, model, minute))
}
pub fn gemini_rpd(provider_id: Uuid, model: &str, date: &str) -> String {
    pk(&d::gemini_rpd_key(provider_id, model, date))
}

// ── Agent pod coordination ───────────────────────────────────────────────────

pub use crate::domain::constants::AGENT_INSTANCES_SET_KEY as AGENT_INSTANCES_SET;
pub fn agent_instances_set() -> String { pk(d::AGENT_INSTANCES_SET_KEY) }
pub fn agent_heartbeat(hostname: &str) -> String { pk(&d::agent_heartbeat_key(hostname)) }

// ── Multi-instance coordination ─────────────────────────────────────────────

pub use crate::domain::constants::INSTANCES_SET_KEY as INSTANCES_SET;
pub fn instances_set() -> String { pk(d::INSTANCES_SET_KEY) }
pub fn heartbeat(instance_id: &str) -> String { pk(&d::heartbeat_key(instance_id)) }
pub fn slot_leases(provider_id: Uuid, model: &str) -> String { pk(&d::slot_leases_key(provider_id, model)) }
pub fn job_owner(job_id: Uuid) -> String { pk(&d::job_owner_key(job_id)) }
pub fn stream_tokens(job_id: Uuid) -> String { pk(&d::stream_tokens_key(job_id)) }
pub fn pubsub_job_events() -> String { pk(d::PUBSUB_JOB_EVENTS_KEY) }
pub fn pubsub_cancel(job_id: Uuid) -> String { pk(&d::pubsub_cancel_key(job_id)) }
pub fn pubsub_cancel_pattern() -> String { pk(d::PUBSUB_CANCEL_PATTERN_KEY) }
pub fn pubsub_cancel_prefix() -> String { pk(d::PUBSUB_CANCEL_PREFIX_KEY) }

// ── Provider liveness (agent heartbeat) ─────────────────────────────────────

pub fn provider_heartbeat(provider_id: Uuid) -> String { pk(&d::provider_heartbeat_key(provider_id)) }
pub fn provider_capacity_state(provider_id: Uuid) -> String { pk(&d::provider_capacity_state_key(provider_id)) }
pub fn providers_online_counter() -> String { pk(d::PROVIDERS_ONLINE_COUNTER_KEY) }
pub fn jobs_pending_counter() -> String { pk(d::JOBS_PENDING_COUNTER_KEY) }
pub fn jobs_running_counter() -> String { pk(d::JOBS_RUNNING_COUNTER_KEY) }

// ── VRAM pool ───────────────────────────────────────────────────────────────

pub fn vram_reserved(provider_id: Uuid) -> String { pk(&d::vram_reserved_key(provider_id)) }
pub fn vram_leases(provider_id: Uuid) -> String { pk(&d::vram_leases_key(provider_id)) }
pub fn vram_leases_scan_pattern() -> String { pk(d::VRAM_LEASES_SCAN_PATTERN_KEY) }

// ── Conversation record cache ────────────────────────────────────────────────

pub fn conversation_record(conversation_id: uuid::Uuid) -> String {
    pk(&d::conversation_record_key(conversation_id))
}
pub fn conv_s3_cache(conv_id: uuid::Uuid) -> String { pk(&d::conv_s3_cache_key(conv_id)) }

// ── Ollama model context cache ───────────────────────────────────────────────

pub fn ollama_model_ctx(provider_id: Uuid, model_name: &str) -> String {
    pk(&d::ollama_model_ctx_key(provider_id, model_name))
}

// ── MCP tool cache ───────────────────────────────────────────────────────────

pub fn mcp_tool(server_id: Uuid) -> String { pk(&d::mcp_tool_key(server_id)) }
pub fn mcp_tool_lock(server_id: Uuid) -> String { pk(&d::mcp_tool_lock_key(server_id)) }
pub fn mcp_heartbeat(server_id: Uuid) -> String { pk(&d::mcp_heartbeat_key(server_id)) }
pub fn mcp_key_acl(api_key_id: Uuid) -> String { pk(&d::mcp_key_acl_key(api_key_id)) }
pub fn mcp_key_cap_points(api_key_id: Uuid) -> String { pk(&d::mcp_key_cap_points_key(api_key_id)) }
pub fn mcp_key_top_k(api_key_id: Uuid) -> String { pk(&d::mcp_key_top_k_key(api_key_id)) }
pub fn mcp_result(tool_name: &str, args_hash: &str) -> String { pk(&d::mcp_result_key(tool_name, args_hash)) }
pub fn mcp_tools_summary(server_id: Uuid) -> String { pk(&d::mcp_tools_summary_key(server_id)) }

// ── Service health (per-instance, written by health_checker) ────────────────

pub fn service_health(instance_id: &str) -> String { pk(&d::service_health_key(instance_id)) }

// ── Placement planner ────────────────────────────────────────────────────────

pub fn preload_lock(model: &str, provider_id: Uuid) -> String { pk(&d::preload_lock_key(model, provider_id)) }
pub fn scaleout_decision(model: &str) -> String { pk(&d::scaleout_decision_key(model)) }

#[cfg(test)]
mod tests {
    use super::*;

    // ── pk() identity / format guards (no init_prefix → pk is no-op) ──────────

    #[test]
    fn pk_no_prefix_is_identity() {
        assert_eq!(pk("veronex:queue:zset"), "veronex:queue:zset");
        assert_eq!(pk(""), "");
    }

    /// Cross-boundary contracts: veronex-agent and veronex-mcp construct the
    /// same key strings independently.  These guards pin the canonical format
    /// (no prefix) so any drift on either side is caught at compile time.

    #[test]
    fn provider_heartbeat_format_matches_agent_convention() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            provider_heartbeat(id),
            "veronex:provider:hb:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn instances_set_value_matches_agent_convention() {
        assert_eq!(INSTANCES_SET, "veronex:instances");
    }

    #[test]
    fn agent_instances_set_matches_agent_convention() {
        assert_eq!(AGENT_INSTANCES_SET, "veronex:agent:instances");
    }

    #[test]
    fn agent_heartbeat_format_matches_agent_convention() {
        assert_eq!(agent_heartbeat("my-host"), "veronex:agent:hb:my-host");
    }

    #[test]
    fn mcp_tool_format() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        assert_eq!(mcp_tool(id), "veronex:mcp:tools:550e8400-e29b-41d4-a716-446655440001");
    }

    #[test]
    fn mcp_tool_lock_format() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();
        assert_eq!(
            mcp_tool_lock(id),
            "veronex:mcp:tools:lock:550e8400-e29b-41d4-a716-446655440002"
        );
    }

    #[test]
    fn mcp_heartbeat_format_matches_agent_and_tool_cache() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        assert_eq!(
            mcp_heartbeat(id),
            "veronex:mcp:heartbeat:550e8400-e29b-41d4-a716-446655440003"
        );
    }

    #[test]
    fn mcp_result_format() {
        assert_eq!(
            mcp_result("mcp_weather_get_weather", "ab12cd34"),
            "veronex:mcp:result:mcp_weather_get_weather:ab12cd34"
        );
    }

    #[test]
    fn queue_key_formats_no_prefix() {
        assert_eq!(queue_zset(),         "veronex:queue:zset");
        assert_eq!(queue_enqueue_at(),   "veronex:queue:enqueue_at");
        assert_eq!(queue_model_map(),    "veronex:queue:model");
        assert_eq!(queue_active(),       "veronex:queue:active");
        assert_eq!(queue_processing(),   "veronex:queue:processing");
    }

    #[test]
    fn pubsub_key_formats_no_prefix() {
        assert_eq!(pubsub_job_events(),    "veronex:pubsub:job_events");
        assert_eq!(pubsub_cancel_pattern(), "veronex:pubsub:cancel:*");
        assert_eq!(pubsub_cancel_prefix(),  "veronex:pubsub:cancel:");
    }

    #[test]
    fn counter_key_formats_no_prefix() {
        assert_eq!(jobs_pending_counter(),    "veronex:stats:jobs:pending");
        assert_eq!(jobs_running_counter(),    "veronex:stats:jobs:running");
        assert_eq!(providers_online_counter(), "veronex:stats:providers:online");
    }

    #[test]
    fn demand_counter_embeds_model_name() {
        assert_eq!(demand_counter("llama3:8b"), "veronex:demand:llama3:8b");
    }

    #[test]
    fn job_owner_format_no_prefix() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            job_owner(id),
            "veronex:job:owner:00000000-0000-0000-0000-000000000000",
        );
    }

    #[test]
    fn ratelimit_tpm_format_no_prefix() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            ratelimit_tpm(id, 1_710_600_000),
            "veronex:ratelimit:tpm:00000000-0000-0000-0000-000000000000:1710600000",
        );
    }

    #[test]
    fn preload_lock_format_no_prefix() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            preload_lock("qwen3:8b", id),
            "veronex:preloading:qwen3:8b:00000000-0000-0000-0000-000000000000",
        );
    }

    #[test]
    fn scaleout_decision_format_no_prefix() {
        assert_eq!(
            scaleout_decision("llama3:70b"),
            "veronex:scaleout:llama3:70b",
        );
    }

    #[test]
    fn cancel_channel_prefix_matches_cancel_channel() {
        let job_id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440099").unwrap();
        let channel = pubsub_cancel(job_id);
        let prefix  = pubsub_cancel_prefix();
        assert!(channel.starts_with(&prefix),
            "channel {channel:?} must start with prefix {prefix:?}");
        assert_eq!(channel.strip_prefix(&prefix), Some("550e8400-e29b-41d4-a716-446655440099"));
    }
}
