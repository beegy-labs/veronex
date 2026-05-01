//! Valkey key registry for all `veronex:*` key patterns.
//!
//! **Key prefix**: call `init_prefix(prefix)` once at startup to prepend a
//! deployment-level prefix to every key (e.g. `"prod:"` → `"prod:veronex:..."`).
//! The default is `""` (no prefix), so existing deployments are unaffected.
//!
//! Queue constants (`QUEUE_*`) are defined in `domain::constants` (the SSOT)
//! and re-exported here for test format-guards.  Runtime code should use the
//! corresponding pk-aware functions (e.g. `queue_zset()` instead of `QUEUE_ZSET`).

use std::sync::OnceLock;
use uuid::Uuid;

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

// ── Queue keys (raw constants — retained for test format guards) ──────────────

pub use crate::domain::constants::{QUEUE_JOBS, QUEUE_JOBS_PAID, QUEUE_JOBS_TEST, QUEUE_PROCESSING};
pub use crate::domain::constants::{QUEUE_ZSET, QUEUE_ENQUEUE_AT, QUEUE_MODEL_MAP};

// ── Queue keys — pk-aware functions for runtime fred calls ───────────────────

pub fn queue_jobs() -> String { format!("{}veronex:queue:jobs", p()) }
pub fn queue_jobs_paid() -> String { format!("{}veronex:queue:jobs:paid", p()) }
pub fn queue_jobs_test() -> String { format!("{}veronex:queue:jobs:test", p()) }
pub fn queue_processing() -> String { format!("{}veronex:queue:processing", p()) }
pub fn queue_active() -> String { format!("{}veronex:queue:active", p()) }
pub fn queue_active_attempts() -> String { format!("{}veronex:queue:active:attempts", p()) }
pub fn queue_zset() -> String { format!("{}veronex:queue:zset", p()) }
pub fn queue_enqueue_at() -> String { format!("{}veronex:queue:enqueue_at", p()) }
pub fn queue_model_map() -> String { format!("{}veronex:queue:model", p()) }

/// Demand counter key for a specific model.
pub fn demand_counter(model: &str) -> String {
    format!("{}veronex:demand:{model}", p())
}

// ── Rate limiting ────────────────────────────────────────────────────────────

/// RPM (requests per minute) sorted-set key for an API key.
pub fn ratelimit_rpm(key_id: Uuid) -> String {
    format!("{}veronex:ratelimit:rpm:{key_id}", p())
}

/// TPM (tokens per minute) counter key for an API key at a given minute epoch.
pub fn ratelimit_tpm(key_id: Uuid, minute: i64) -> String {
    format!("{}veronex:ratelimit:tpm:{key_id}:{minute}", p())
}

// ── Auth / session ───────────────────────────────────────────────────────────

/// Key for a revoked JWT (stored until natural expiry).
pub fn revoked_jti(jti: Uuid) -> String {
    format!("{}veronex:revoked:{jti}", p())
}

/// Key for a password-reset token (24 h TTL).
pub fn password_reset(token: &str) -> String {
    format!("{}veronex:pwreset:{token}", p())
}

/// Key for a used refresh token hash (prevents replay attacks).
pub fn refresh_blocklist(hash: &str) -> String {
    format!("{}veronex:refresh_used:{hash}", p())
}

/// IP-based login attempt counter (5-minute sliding window).
pub fn login_attempts(ip: &str) -> String {
    format!("{}veronex:login_attempts:{ip}", p())
}

// ── Provider infrastructure ──────────────────────────────────────────────────

/// Thermal throttle level cache for a provider.
pub fn thermal_throttle(provider_id: Uuid) -> String {
    format!("{}veronex:throttle:{provider_id}", p())
}

/// Cached Ollama model list for a provider.
pub fn provider_models(provider_id: Uuid) -> String {
    format!("{}veronex:models:{provider_id}", p())
}

/// Hardware metrics cache for a provider's GPU server.
pub fn hw_metrics(provider_id: Uuid) -> String {
    format!("{}veronex:hw:{provider_id}", p())
}

/// Full node-exporter metrics cache for a GPU server.
/// Cached by health_checker, read by dashboard API.
pub fn server_node_metrics(server_id: Uuid) -> String {
    format!("{}veronex:server_metrics:{server_id}", p())
}

// ── Gemini rate-limit counters ───────────────────────────────────────────────

/// Gemini RPM counter (per provider + model + minute).
pub fn gemini_rpm(provider_id: Uuid, model: &str, minute: i64) -> String {
    format!("{}veronex:gemini:rpm:{provider_id}:{model}:{minute}", p())
}

/// Gemini RPD counter (per provider + model + date).
pub fn gemini_rpd(provider_id: Uuid, model: &str, date: &str) -> String {
    format!("{}veronex:gemini:rpd:{provider_id}:{model}:{date}", p())
}

// ── Agent pod coordination ───────────────────────────────────────────────────

/// SET of all veronex-agent hostnames — raw constant for test format guards.
/// Runtime code: use `agent_instances_set()` for the pk-aware version.
pub const AGENT_INSTANCES_SET: &str = "veronex:agent:instances";

/// pk-aware version of `AGENT_INSTANCES_SET` for runtime fred calls.
pub fn agent_instances_set() -> String {
    format!("{}veronex:agent:instances", p())
}

/// Heartbeat key for a veronex-agent pod (EX 180s, refreshed every 60s).
/// Written by: veronex-agent. Read by: dashboard GET /v1/dashboard/pods.
pub fn agent_heartbeat(hostname: &str) -> String {
    format!("{}veronex:agent:hb:{hostname}", p())
}

// ── Multi-instance coordination ─────────────────────────────────────────────

/// SET of all API instance IDs — raw constant for test format guards.
/// Runtime code: use `instances_set()` for the pk-aware version.
pub const INSTANCES_SET: &str = "veronex:instances";

/// pk-aware version of `INSTANCES_SET` for runtime fred calls.
pub fn instances_set() -> String {
    format!("{}veronex:instances", p())
}

/// Instance heartbeat key (EX 30s, refreshed every 10s).
pub fn heartbeat(instance_id: &str) -> String {
    format!("{}veronex:heartbeat:{instance_id}", p())
}

/// ZSET of slot leases for crash recovery.
/// Members: `{instance_id}:{lease_id}`, scores: expiry timestamp.
pub fn slot_leases(provider_id: Uuid, model: &str) -> String {
    format!("{}veronex:slot_leases:{provider_id}:{model}", p())
}

/// Tracks which instance owns a running job (EX 300s).
pub fn job_owner(job_id: Uuid) -> String {
    format!("{}veronex:job:owner:{job_id}", p())
}

/// Valkey Stream key for cross-instance token relay (XADD/XREAD).
///
/// Uses Streams instead of Pub/Sub to prevent initial token black hole —
/// late-connecting subscribers can read from `0-0` to catch up.
pub fn stream_tokens(job_id: Uuid) -> String {
    format!("{}veronex:stream:tokens:{job_id}", p())
}

/// Pub/sub channel for cross-instance job status events — pk-aware.
pub fn pubsub_job_events() -> String {
    format!("{}veronex:pubsub:job_events", p())
}

/// Pub/sub channel for cross-instance cancellation signals.
pub fn pubsub_cancel(job_id: Uuid) -> String {
    format!("{}veronex:pubsub:cancel:{job_id}", p())
}

/// Pattern for subscribing to all cancel channels — pk-aware.
pub fn pubsub_cancel_pattern() -> String {
    format!("{}veronex:pubsub:cancel:*", p())
}

// ── Provider liveness (agent heartbeat) ─────────────────────────────────────

/// Heartbeat key set by veronex-agent after each successful Ollama scrape.
/// TTL = 3× scrape interval (default 180s). Missing key = provider offline.
/// Written by: veronex-agent. Read by: health_checker (MGET batch).
pub fn provider_heartbeat(provider_id: Uuid) -> String {
    format!("{}veronex:provider:hb:{provider_id}", p())
}

/// Capacity state pushed by veronex-agent: loaded models + arch profiles + total_vram_mb.
/// TTL = 3× scrape interval (default 180s). Written by: agent. Read by: analyzer sync_loop.
pub fn provider_capacity_state(provider_id: Uuid) -> String {
    format!("{}veronex:provider:{provider_id}:capacity_state", p())
}

/// Global O(1) counter of currently-online Ollama providers — pk-aware.
pub fn providers_online_counter() -> String {
    format!("{}veronex:stats:providers:online", p())
}

/// Atomic counter of pending jobs — pk-aware.
pub fn jobs_pending_counter() -> String {
    format!("{}veronex:stats:jobs:pending", p())
}

/// Atomic counter of running jobs — pk-aware.
pub fn jobs_running_counter() -> String {
    format!("{}veronex:stats:jobs:running", p())
}

// ── VRAM pool ───────────────────────────────────────────────────────────────

/// Valkey key tracking total reserved VRAM (MB) per provider.
pub fn vram_reserved(provider_id: Uuid) -> String {
    format!("{}veronex:vram_reserved:{provider_id}", p())
}

/// ZSET of VRAM lease entries for crash recovery.
pub fn vram_leases(provider_id: Uuid) -> String {
    format!("{}veronex:vram_leases:{provider_id}", p())
}

/// Scan pattern matching all VRAM lease ZSETs — pk-aware.
pub fn vram_leases_scan_pattern() -> String {
    format!("{}veronex:vram_leases:*", p())
}

// ── Conversation record cache ────────────────────────────────────────────────

/// Cached ConversationRecord for a multi-turn session (zstd-compressed JSON).
/// TTL = 300s. Written by: runner.rs after S3 put_conversation().
/// Invalidated (DEL) by: compress_turn() after S3 re-write.
pub fn conversation_record(conversation_id: uuid::Uuid) -> String {
    format!("{}veronex:conv:{conversation_id}", p())
}

/// Cached S3 conversation detail (full turn list) for conversation_handlers.
/// TTL = 300s. Written by: fetch_conv_s3_cached(). Invalidated by: MCP bridge after S3 re-write.
pub fn conv_s3_cache(conv_id: uuid::Uuid) -> String {
    format!("{}conv_s3:{conv_id}", p())
}

// ── Ollama model context cache ───────────────────────────────────────────────

/// Cached Ollama model context window profile.
/// Value: JSON `{"configured_ctx": 4096, "max_ctx": 8192}`. TTL = 600s.
/// Written by: capacity analyzer after DB upsert. Read by: OllamaAdapter inference hot-path.
pub fn ollama_model_ctx(provider_id: Uuid, model_name: &str) -> String {
    format!("{}veronex:ollama:ctx:{provider_id}:{model_name}", p())
}


// ── MCP tool cache ───────────────────────────────────────────────────────────

/// Cached tool schema for a single MCP server (JSON-serialized `Vec<McpTool>`).
/// TTL = 35 s. Written by: veronex (SET NX leader). Read by: all veronex replicas.
pub fn mcp_tool(server_id: Uuid) -> String {
    format!("{}veronex:mcp:tools:{server_id}", p())
}

/// Refresh lock for a single MCP server — prevents thundering herd.
/// TTL = 33 s. Held by the replica that won the SET NX race.
pub fn mcp_tool_lock(server_id: Uuid) -> String {
    format!("{}veronex:mcp:tools:lock:{server_id}", p())
}

/// MCP server liveness heartbeat set by veronex-agent.
/// TTL = 3× scrape interval (default 180 s). Missing key = server offline.
/// Written by: veronex-agent. Read by: McpToolCache::is_online().
pub fn mcp_heartbeat(server_id: Uuid) -> String {
    format!("{}veronex:mcp:heartbeat:{server_id}", p())
}

/// Per-API-key MCP server allowlist cache.
/// Value: JSON array of allowed server UUIDs, e.g. `["uuid1","uuid2"]` or `[]`.
/// Empty array = no MCP access (default deny). TTL = 60s.
/// Invalidated on grant/revoke in key_mcp_access_handlers.
pub fn mcp_key_acl(api_key_id: Uuid) -> String {
    format!("{}veronex:mcp:acl:{api_key_id}", p())
}

/// Cached mcp_cap_points for an API key (u8 as string). TTL = 60s.
/// Invalidated on key update.
pub fn mcp_key_cap_points(api_key_id: Uuid) -> String {
    format!("{}veronex:mcp:cap:{api_key_id}", p())
}

/// Cached MIN(top_k) across mcp_key_access rows for an API key (u16 as string). TTL = 60s.
/// Invalidated on grant/revoke in key_mcp_access_handlers.
pub fn mcp_key_top_k(api_key_id: Uuid) -> String {
    format!("{}veronex:mcp:topk:{api_key_id}", p())
}

/// Cached MCP tool result keyed by (tool_name, args_hash).
/// TTL is tool-specific (readOnlyHint + idempotentHint condition).
/// Written + read by: veronex McpResultCache.
pub fn mcp_result(tool_name: &str, args_hash: &str) -> String {
    format!("{}veronex:mcp:result:{tool_name}:{args_hash}", p())
}

/// Cached MCP tool summary (condensed schema for context injection).
/// TTL = 3600 s. Written by: mcp_handlers on tool refresh. Read by: list endpoint cache.
pub fn mcp_tools_summary(server_id: Uuid) -> String {
    format!("{}veronex:mcp:tools_summary:{server_id}", p())
}

// ── Pub/sub prefix helpers ───────────────────────────────────────────────────

/// String prefix of all cancel pub/sub channels — pk-aware.
/// Used by relay.rs to strip the prefix and extract the job_id.
/// Must match the prefix produced by `pubsub_cancel()`.
pub fn pubsub_cancel_prefix() -> String {
    format!("{}veronex:pubsub:cancel:", p())
}

// ── Service health (per-instance, written by health_checker) ────────────────

/// Per-instance infrastructure service health HASH.
/// Fields: service_name → JSON `{"s":"ok","ms":3,"t":1711699200000}`.
/// TTL = 60 s (2× health check interval). Dead pod → auto-expire.
pub fn service_health(instance_id: &str) -> String {
    format!("{}veronex:svc:health:{instance_id}", p())
}

// ── Placement planner ────────────────────────────────────────────────────────

pub fn preload_lock(model: &str, provider_id: Uuid) -> String {
    format!("{}veronex:preloading:{model}:{provider_id}", p())
}

pub fn scaleout_decision(model: &str) -> String {
    format!("{}veronex:scaleout:{model}", p())
}

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

    // ── Key prefix format guards ──────────────────────────────────────────────
    // Tests run without init_prefix(), so KEY_PREFIX is unset → p() returns "".
    // These guards pin key formats AND verify pk() is a no-op when prefix is empty.

    /// pk() must be identity when no prefix is configured.
    #[test]
    fn pk_no_prefix_is_identity() {
        assert_eq!(pk("veronex:queue:zset"), "veronex:queue:zset");
        assert_eq!(pk(""), "");
    }

    /// Queue key format guards for cross-boundary contracts (veronex-agent reads these).
    #[test]
    fn queue_key_formats_no_prefix() {
        assert_eq!(queue_zset(),         "veronex:queue:zset");
        assert_eq!(queue_enqueue_at(),   "veronex:queue:enqueue_at");
        assert_eq!(queue_model_map(),    "veronex:queue:model");
        assert_eq!(queue_active(),       "veronex:queue:active");
        assert_eq!(queue_processing(),   "veronex:queue:processing");
    }

    /// Pub/sub key format guards — relay.rs and cancel-subscriber strip these prefixes.
    #[test]
    fn pubsub_key_formats_no_prefix() {
        assert_eq!(pubsub_job_events(),    "veronex:pubsub:job_events");
        assert_eq!(pubsub_cancel_pattern(), "veronex:pubsub:cancel:*");
        assert_eq!(pubsub_cancel_prefix(),  "veronex:pubsub:cancel:");
    }

    /// Stat counter key formats — background.rs seeds these at startup.
    #[test]
    fn counter_key_formats_no_prefix() {
        assert_eq!(jobs_pending_counter(),    "veronex:stats:jobs:pending");
        assert_eq!(jobs_running_counter(),    "veronex:stats:jobs:running");
        assert_eq!(providers_online_counter(), "veronex:stats:providers:online");
    }

    /// demand_counter must embed the model name — demand_resync relies on exact format.
    #[test]
    fn demand_counter_embeds_model_name() {
        assert_eq!(demand_counter("llama3:8b"), "veronex:demand:llama3:8b");
    }

    /// job_owner key format — pinned for cross-instance ownership lookup.
    #[test]
    fn job_owner_format_no_prefix() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            job_owner(id),
            "veronex:job:owner:00000000-0000-0000-0000-000000000000",
        );
    }

    /// ratelimit_tpm key format — embeds key_id and minute epoch.
    #[test]
    fn ratelimit_tpm_format_no_prefix() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            ratelimit_tpm(id, 1_710_600_000),
            "veronex:ratelimit:tpm:00000000-0000-0000-0000-000000000000:1710600000",
        );
    }

    /// preload_lock key format — embeds model and provider_id.
    #[test]
    fn preload_lock_format_no_prefix() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            preload_lock("qwen3:8b", id),
            "veronex:preloading:qwen3:8b:00000000-0000-0000-0000-000000000000",
        );
    }

    /// scaleout_decision key format — embeds the model name.
    #[test]
    fn scaleout_decision_format_no_prefix() {
        assert_eq!(
            scaleout_decision("llama3:70b"),
            "veronex:scaleout:llama3:70b",
        );
    }

    /// pubsub_cancel_prefix must be a strict prefix of pubsub_cancel(job_id).
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
