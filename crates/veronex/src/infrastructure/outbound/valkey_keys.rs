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

/// Full node-exporter metrics cache for a GPU server.
/// Cached by health_checker, read by dashboard API.
pub fn server_node_metrics(server_id: Uuid) -> String {
    format!("veronex:server_metrics:{server_id}")
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

// ── Agent pod coordination ───────────────────────────────────────────────────

/// SET of all veronex-agent hostnames (SADD/SREM on heartbeat refresh).
/// Written by: veronex-agent. Read by: dashboard GET /v1/dashboard/pods.
/// Cross-boundary: veronex-agent cannot import this module — must maintain
/// the same string in agent's heartbeat.rs. Format is pinned by test below.
pub const AGENT_INSTANCES_SET: &str = "veronex:agent:instances";

/// Heartbeat key for a veronex-agent pod (EX 180s, refreshed every 60s).
/// Written by: veronex-agent. Read by: dashboard GET /v1/dashboard/pods.
pub fn agent_heartbeat(hostname: &str) -> String {
    format!("veronex:agent:hb:{hostname}")
}

// ── Multi-instance coordination ─────────────────────────────────────────────

/// SET of all API instance IDs (SADD on heartbeat refresh).
/// Used by orphan sweeper to enumerate all known instances.
pub const INSTANCES_SET: &str = "veronex:instances";

/// Instance heartbeat key (EX 30s, refreshed every 10s).
pub fn heartbeat(instance_id: &str) -> String {
    format!("veronex:heartbeat:{instance_id}")
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
/// TTL = 3× scrape interval (default 180s). Missing key = provider offline.
/// Written by: veronex-agent. Read by: health_checker (MGET batch).
pub fn provider_heartbeat(provider_id: Uuid) -> String {
    format!("veronex:provider:hb:{provider_id}")
}

/// Capacity state pushed by veronex-agent: loaded models + arch profiles + total_vram_mb.
/// TTL = 3× scrape interval (default 180s). Written by: agent. Read by: analyzer sync_loop.
pub fn provider_capacity_state(provider_id: Uuid) -> String {
    format!("veronex:provider:{provider_id}:capacity_state")
}

/// Global O(1) counter of currently-online Ollama providers.
/// Incremented/decremented atomically by health_checker on status transitions.
/// Read by dashboard to avoid SELECT COUNT(*) from DB.
pub const PROVIDERS_ONLINE_COUNTER: &str = "veronex:stats:providers:online";

/// Atomic counter of pending jobs (INCR on create, DECR on dispatch/cancel/fail).
/// Reconciled from DB every 60 ticks. Read by stats ticker instead of DB query.
pub const JOBS_PENDING_COUNTER: &str = "veronex:stats:jobs:pending";

/// Atomic counter of running jobs (INCR on dispatch start, DECR on complete/fail/cancel).
/// Reconciled from DB every 60 ticks. Read by stats ticker instead of DB query.
pub const JOBS_RUNNING_COUNTER: &str = "veronex:stats:jobs:running";

// ── VRAM pool ───────────────────────────────────────────────────────────────

/// Valkey key tracking total reserved VRAM (MB) per provider.
pub fn vram_reserved(provider_id: Uuid) -> String {
    format!("veronex:vram_reserved:{provider_id}")
}

/// ZSET of VRAM lease entries for crash recovery.
pub fn vram_leases(provider_id: Uuid) -> String {
    format!("veronex:vram_leases:{provider_id}")
}

/// Scan pattern matching all VRAM lease ZSETs (used by reap_all_expired).
pub const VRAM_LEASES_SCAN_PATTERN: &str = "veronex:vram_leases:*";

// ── Conversation record cache ────────────────────────────────────────────────

/// Cached ConversationRecord for a multi-turn session (zstd-compressed JSON).
/// TTL = 300s. Written by: runner.rs after S3 put_conversation().
/// Invalidated (DEL) by: compress_turn() after S3 re-write.
pub fn conversation_record(conversation_id: uuid::Uuid) -> String {
    format!("veronex:conv:{conversation_id}")
}

// ── Ollama model context cache ───────────────────────────────────────────────

/// Cached Ollama model context window profile.
/// Value: JSON `{"configured_ctx": 4096, "max_ctx": 8192}`. TTL = 600s.
/// Written by: capacity analyzer after DB upsert. Read by: OllamaAdapter inference hot-path.
pub fn ollama_model_ctx(provider_id: Uuid, model_name: &str) -> String {
    format!("veronex:ollama:ctx:{provider_id}:{model_name}")
}

// ── MCP tool cache ───────────────────────────────────────────────────────────

/// Cached tool schema for a single MCP server (JSON-serialized `Vec<McpTool>`).
/// TTL = 35 s. Written by: veronex (SET NX leader). Read by: all veronex replicas.
pub fn mcp_tool(server_id: Uuid) -> String {
    format!("veronex:mcp:tools:{server_id}")
}

/// Refresh lock for a single MCP server — prevents thundering herd.
/// TTL = 33 s. Held by the replica that won the SET NX race.
pub fn mcp_tool_lock(server_id: Uuid) -> String {
    format!("veronex:mcp:tools:lock:{server_id}")
}

/// MCP server liveness heartbeat set by veronex-agent.
/// TTL = 3× scrape interval (default 180 s). Missing key = server offline.
/// Written by: veronex-agent. Read by: McpToolCache::is_online().
pub fn mcp_heartbeat(server_id: Uuid) -> String {
    format!("veronex:mcp:heartbeat:{server_id}")
}

/// Per-API-key MCP server allowlist cache.
/// Value: JSON array of allowed server UUIDs, e.g. `["uuid1","uuid2"]` or `[]`.
/// Empty array = no MCP access (default deny). TTL = 60s.
/// Invalidated on grant/revoke in key_mcp_access_handlers.
pub fn mcp_key_acl(api_key_id: Uuid) -> String {
    format!("veronex:mcp:acl:{api_key_id}")
}

/// Cached mcp_cap_points for an API key (u8 as string). TTL = 60s.
/// Invalidated on key update.
pub fn mcp_key_cap_points(api_key_id: Uuid) -> String {
    format!("veronex:mcp:cap:{api_key_id}")
}

/// Cached MIN(top_k) across mcp_key_access rows for an API key (u16 as string). TTL = 60s.
/// Invalidated on grant/revoke in key_mcp_access_handlers.
pub fn mcp_key_top_k(api_key_id: Uuid) -> String {
    format!("veronex:mcp:topk:{api_key_id}")
}

/// Cached MCP tool result keyed by (tool_name, args_hash).
/// TTL is tool-specific (readOnlyHint + idempotentHint condition).
/// Written + read by: veronex McpResultCache.
pub fn mcp_result(tool_name: &str, args_hash: &str) -> String {
    format!("veronex:mcp:result:{tool_name}:{args_hash}")
}

/// Cached MCP tool summary (condensed schema for context injection).
/// TTL = 3600 s. Written by: mcp_handlers on tool refresh. Read by: list endpoint cache.
pub fn mcp_tools_summary(server_id: Uuid) -> String {
    format!("veronex:mcp:tools_summary:{server_id}")
}

// ── Pub/sub prefix helpers ───────────────────────────────────────────────────

/// String prefix of all cancel pub/sub channels.
/// Used by relay.rs to strip the prefix and extract the job_id.
/// Must match the prefix produced by `pubsub_cancel()`.
pub const PUBSUB_CANCEL_PREFIX: &str = "veronex:pubsub:cancel:";

// ── Service health (per-instance, written by health_checker) ────────────────

/// Per-instance infrastructure service health HASH.
/// Fields: service_name → JSON `{"s":"ok","ms":3,"t":1711699200000}`.
/// TTL = 60 s (2× health check interval). Dead pod → auto-expire.
pub fn service_health(instance_id: &str) -> String {
    format!("veronex:svc:health:{instance_id}")
}

// ── Placement planner (SSOT in domain::constants) ───────────────────────────

pub use crate::domain::constants::{preload_lock_key as preload_lock};
pub use crate::domain::constants::{scaleout_decision_key as scaleout_decision};

#[cfg(test)]
mod tests {
    use super::*;

    /// provider_heartbeat() must produce the canonical format consumed by the
    /// agent's heartbeat::key() and by MGET in health_checker.
    /// Guards against crate-boundary drift between veronex and veronex-agent.
    #[test]
    fn provider_heartbeat_format_matches_agent_convention() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            provider_heartbeat(id),
            "veronex:provider:hb:550e8400-e29b-41d4-a716-446655440000"
        );
    }

    /// INSTANCES_SET must match the hardcoded key in veronex-agent's orphan_sweeper.
    /// Guards against crate-boundary drift since agent cannot import this module.
    #[test]
    fn instances_set_value_matches_agent_convention() {
        assert_eq!(INSTANCES_SET, "veronex:instances");
    }

    // ── MCP cross-boundary key format guards ──────────────────────────────────
    // veronex-mcp and veronex-agent cannot import from this module, so they
    // construct the same key strings independently.  These tests pin the format
    // so any drift is caught at compile time on either side.

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

    /// mcp_heartbeat() must match what veronex-agent's scraper writes:
    ///   `format!("veronex:mcp:heartbeat:{server_id}")`
    /// and what veronex-mcp's McpToolCache::is_online() reads.
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

    /// AGENT_INSTANCES_SET must match the constant in veronex-agent's heartbeat.rs.
    /// Guards against cross-crate key drift since agent cannot import this module.
    #[test]
    fn agent_instances_set_matches_agent_convention() {
        assert_eq!(AGENT_INSTANCES_SET, "veronex:agent:instances");
    }

    /// agent_heartbeat() must match the key format in veronex-agent's heartbeat.rs.
    #[test]
    fn agent_heartbeat_format_matches_agent_convention() {
        assert_eq!(agent_heartbeat("my-host"), "veronex:agent:hb:my-host");
    }
}
