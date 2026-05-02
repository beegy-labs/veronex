//! Domain and application-layer constants.
//!
//! Lives in the domain layer so both `application::use_cases` and
//! `infrastructure` can import without violating hexagonal dependency rules.
//! HTTP-specific constants (SSE, models cache) remain in
//! `infrastructure::inbound::http::constants`.

use std::time::Duration;

// ── Tier / routing strings ───────────────────────────────────────────────────

/// Gemini free-tier routing value.
pub const GEMINI_TIER_FREE: &str = "free";

/// API key billing tier value for paid keys.
pub const KEY_TIER_PAID: &str = "paid";

/// Prefix prepended to every generated API key plaintext (e.g. `vnx_<base62>`).
pub const API_KEY_PREFIX: &str = "vnx_";

// ── TPM rate limiting ────────────────────────────────────────────────────────

/// Estimated tokens reserved per request at admission by the rate limiter.
///
/// The rate limiter pre-charges this amount; after job completion `record_tpm`
/// adjusts by `actual_tokens - TPM_ESTIMATED_TOKENS`.  **This constant must
/// be identical** in the rate limiter and the TPM reconciliation path — hence
/// it lives here as the single source of truth.
pub const TPM_ESTIMATED_TOKENS: i64 = 500;

// ── Inference job lifecycle ──────────────────────────────────────────────────

/// Delay before removing a completed/failed JobEntry from the in-memory DashMap.
///
/// Keeps tokens available for late-connecting SSE clients, then frees memory.
pub const JOB_CLEANUP_DELAY: Duration = Duration::from_secs(60);

/// Shorter cleanup delay when ownership is lost (another instance took over).
pub const OWNERSHIP_LOST_CLEANUP_DELAY: Duration = Duration::from_secs(5);

/// Empty-queue poll interval in the queue dispatcher loop.
pub const QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Backoff when no provider is available to handle a popped job.
pub const NO_PROVIDER_BACKOFF: Duration = Duration::from_secs(2);

/// Backoff after a queue pop error.
pub const QUEUE_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// TTL for the `veronex:job:owner:{job_id}` Valkey key.
pub const JOB_OWNER_TTL_SECS: i64 = 300;

/// How often `run_job` refreshes the owner key to prevent false reaper re-enqueue.
pub const OWNER_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// Initial capacity for the per-job token vector.
pub const INITIAL_TOKEN_CAPACITY: usize = 256;

/// Hard limit on tokens stored per job to prevent unbounded memory growth.
///
/// If a job produces more than this many tokens, it is force-terminated.
/// 100k tokens is well beyond any realistic inference response.
pub const MAX_TOKENS_PER_JOB: usize = 100_000;

// ── Queue key names (used by inference use case) ────────────────────────────

/// Default API job queue.
pub const QUEUE_JOBS: &str = "veronex:queue:jobs";

/// Paid-tier API job queue (highest priority — polled first by BLPOP).
pub const QUEUE_JOBS_PAID: &str = "veronex:queue:jobs:paid";

/// Test/dashboard job queue (lowest priority).
pub const QUEUE_JOBS_TEST: &str = "veronex:queue:jobs:test";

/// Processing list for reliable queue (BLMOVE destination).
pub const QUEUE_PROCESSING: &str = "veronex:queue:processing";

/// Active-processing ZSET — score = lease deadline (unix_ms).
pub const QUEUE_ACTIVE: &str = "veronex:queue:active";

/// Hash tracking re-enqueue attempt counts for lease-expired jobs.
pub const QUEUE_ACTIVE_ATTEMPTS: &str = "veronex:queue:active:attempts";

/// Counter prefix for "no eligible provider" retries per job.
/// Key format: `{NO_PROVIDER_ATTEMPTS_PREFIX}:{job_id}`.
pub const NO_PROVIDER_ATTEMPTS_PREFIX: &str = "veronex:queue:no_provider_attempts";

/// Max consecutive "no eligible provider" cycles before a job is permanently failed.
/// Each cycle = one dispatcher tick (~poll_interval). Default 3 tries ≈ ≤3s.
pub const MAX_NO_PROVIDER_ATTEMPTS: i64 = 3;

/// Lease TTL in ms. Workers must renew before expiry or job is reaped.
pub const LEASE_TTL_MS: u64 = 90_000;

/// How often (secs) a worker renews the active lease.
pub const LEASE_RENEW_INTERVAL_SECS: u64 = 30;

/// How often (secs) the processing reaper runs.
pub const PROCESSING_REAPER_SECS: u64 = 30;

/// Max orphan recoveries before a job is permanently failed.
pub const LEASE_MAX_ATTEMPTS: u64 = 2;

/// Scoring bonus (MB) for models already loaded in VRAM (locality preference).
pub const MODEL_LOCALITY_BONUS_MB: i64 = 100_000;

// ── ZSET queue (Phase 3) ──────────────────────────────────────────────────

/// Unified priority queue (ZSET). Lower score = higher priority.
pub const QUEUE_ZSET: &str = "veronex:queue:zset";

/// Side hash: job_id → enqueue_at_ms (for promote_overdue & age_bonus).
pub const QUEUE_ENQUEUE_AT: &str = "veronex:queue:enqueue_at";

/// Side hash: job_id → model (for demand_resync).
pub const QUEUE_MODEL_MAP: &str = "veronex:queue:model";

/// Hard cap on ZSET queue size. Enqueue returns 429 when exceeded.
pub const MAX_QUEUE_SIZE: u64 = 10_000;

/// Per-model queue cap via demand counter. Prevents hot-model monopoly.
pub const MAX_QUEUE_PER_MODEL: u64 = 2_000;

/// Tier bonus (ms) subtracted from enqueue timestamp for paid-tier jobs.
pub const TIER_BONUS_PAID: u64 = 300_000;

/// Tier bonus (ms) for standard (free API key) tier.
pub const TIER_BONUS_STANDARD: u64 = 100_000;

/// Tier bonus (ms) for test/dashboard jobs.
pub const TIER_BONUS_TEST: u64 = 0;

/// After this many seconds in queue, promote_overdue applies EMERGENCY_BONUS.
pub const TIER_EXPIRE_SECS: u64 = 250;

/// Locality bonus (ms) in dispatcher scoring for already-loaded models.
pub const LOCALITY_BONUS_MS: f64 = 20_000.0;

/// Emergency bonus applied to overdue jobs (= TIER_BONUS_PAID).
pub const EMERGENCY_BONUS_MS: u64 = 300_000;

/// Interval for the promote_overdue background loop.
pub const OVERDUE_PROMOTE_SECS: u64 = 30;

/// Interval for the demand_resync background loop.
pub const DEMAND_RESYNC_SECS: u64 = 60;

/// Max time a job may wait in the ZSET queue before being auto-cancelled (§7).
pub const MAX_QUEUE_WAIT_SECS: u64 = 300;

/// Interval for the queue_wait_cancel background loop.
pub const QUEUE_WAIT_CANCEL_SECS: u64 = 30;

/// Default top-K window for ZSET peek in dispatcher.
pub const ZSET_PEEK_K: u64 = 20;

/// Maximum top-K window (adaptive scaling when queue is large).
pub const ZSET_PEEK_K_MAX: u64 = 100;

// ── Input safety limits (LLM gateway — GPU monopoly / context bomb prevention) ─

/// Hard cap on `max_tokens` / `max_completion_tokens` accepted from clients.
///
/// Prevents a single request from monopolizing GPU memory for an unbounded
/// generation window.  Must be identical across every handler that caps
/// `max_tokens` — defined here as the single source of truth.
pub const MAX_TOKENS_CEILING: u32 = 32_768;

/// Maximum number of messages accepted in a chat request (context bomb guard).
///
/// Unbounded `messages` arrays inflate KV-cache linearly.  Enforced at the
/// HTTP boundary before any token or VRAM accounting occurs.
pub const MAX_CHAT_MESSAGES: usize = 256;

// ── Streaming buffer limits ─────────────────────────────────────────────────

/// Maximum bytes allowed in an SSE/NDJSON line buffer before aborting.
///
/// Shared by Ollama (NDJSON) and Gemini (SSE) streaming adapters.
pub const MAX_LINE_BUFFER: usize = 1_048_576; // 1 MB

// ── HTTP request timeouts ──────────────────────────────────────────────────

/// Timeout for inference requests to Ollama/Gemini providers (5 min).
pub const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout for Ollama API metadata calls (/api/show, /api/tags, /api/ps).
pub const OLLAMA_METADATA_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for Ollama health check (/api/version).
pub const OLLAMA_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for Gemini health check (lightweight models list).
pub const GEMINI_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for LLM single-model analysis call.
pub const LLM_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for LLM batch analysis call (all models).
pub const LLM_BATCH_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(60);

/// Timeout for node-exporter metrics fetch.
pub const NODE_EXPORTER_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for job cancellation in CancelGuard.
pub const CANCEL_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for an infrastructure service probe (PostgreSQL SELECT 1, ClickHouse /ping, etc.).
/// Used by health_checker to classify services as "ok" or "error" in the dashboard.
pub const SERVICE_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Periodic MCP tool-discovery refresh interval in the background task in main.rs.
pub const MCP_TOOL_REFRESH_INTERVAL: Duration = Duration::from_secs(25);

// ── MCP / Ollama lifecycle phase timeouts ────────────────────────────────────
//
// The Phase-1 cold-load timeout is observed in two layers and must stay
// coupled or runtime races. Defined here as the single source of truth.
//
// - `ollama::lifecycle::probe_load` sets `reqwest::timeout(...)` to bound the
//   cold-load HTTP request (200K-context measured ≈248 s on Strix Halo + ROCm).
// - `mcp::bridge` uses the same value as the Phase-1 wait for the runner's
//   `phase_boundary` token. If the bridge wait is shorter, the bridge fails the
//   round before the load finishes; if longer, the bridge stalls past the load
//   timeout.
//
// SDD: `.specs/veronex/bridge-phase-aware-timing.md` §3.2.

/// Phase-1 cold-load timeout (lifecycle reqwest + bridge phase wait).
/// 600 s envelope covers measured worst-case 248 s on `qwen3-coder-next-200k`
/// (Strix Halo) with ~2.4× headroom for future 300K+ context models or
/// VRAM-scheduler congestion.
pub const MCP_LIFECYCLE_LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Phase-2 first-token timeout — applies after `phase_boundary` arrives.
/// Originally 60 s; bumped to 300 s after live verify on
/// `qwen3-coder-next-200k:latest` observed Phase-2 first-token NOT being
/// sub-second when prefill is large (200K-context + ~5K MCP-injected tokens).
pub const MCP_TOKEN_FIRST_TIMEOUT: Duration = Duration::from_secs(300);

/// Per-token stream idle. Fires only when the model hangs mid-response
/// (true stall). Generation gap on warm models is sub-second; 45 s is a
/// generous safety margin.
pub const MCP_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// Hard cap per MCP round. Must be ≥ `MCP_LIFECYCLE_LOAD_TIMEOUT` +
/// `MCP_TOKEN_FIRST_TIMEOUT` plus a streaming budget, AND ≤ Cilium HTTPRoute
/// `timeouts.request` (1800 s). 1500 s = 600 (Phase 1) + 300 (Phase 2 first
/// token) + 600 streaming budget, leaving 300 s headroom under the gateway cap.
pub const MCP_ROUND_TOTAL_TIMEOUT: Duration = Duration::from_secs(1500);

// ── Cache TTL ──────────────────────────────────────────────────────────────

/// TTL for per-provider HwMetrics in Valkey (seconds).
pub const HW_METRICS_TTL: i64 = 60;

/// TTL for per-server NodeMetrics in Valkey (seconds).
pub const NODE_METRICS_TTL: i64 = 60;

/// TTL for OllamaModel provider-for-model lookup cache (hot path).
pub const OLLAMA_MODEL_CACHE_TTL: Duration = Duration::from_secs(10);

/// TTL for provider-model-selection enabled list cache.
pub const MODEL_SELECTION_CACHE_TTL: Duration = Duration::from_secs(30);

/// TTL for the CachingProviderRegistry in-memory snapshot.
pub const PROVIDER_REGISTRY_CACHE_TTL: Duration = Duration::from_secs(5);

/// TTL for the per-hash API key cache (hot path: every inference request).
/// Mutations (revoke, deactivate, soft-delete, regenerate) call
/// `invalidate_all()` so stale entries are evicted immediately on any admin
/// operation. 60 s collapses repeated DB lookups for the same key into one
/// hit per TTL period.
pub const API_KEY_CACHE_TTL: Duration = Duration::from_secs(60);

/// TTL for the lab settings cache. lab_settings is a single admin-only row
/// updated rarely; `update()` calls `invalidate_all()` so new values are
/// visible on the next request. 30 s balances freshness vs DB load.
pub const LAB_SETTINGS_CACHE_TTL: Duration = Duration::from_secs(30);

// ── Sync / sweep intervals ────────────────────────────────────────────────

/// Base tick interval for the capacity analyzer sync loop.
pub const SYNC_LOOP_BASE_TICK: Duration = Duration::from_secs(30);

/// Interval between health checker passes (provider liveness probes).
pub const HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

/// Interval between pending-job sweep passes (reaper).
pub const PENDING_JOB_SWEEP_INTERVAL: Duration = Duration::from_secs(300);

/// Tick interval for the real-time FlowStats broadcast ticker.
pub const STATS_TICK_INTERVAL: Duration = Duration::from_secs(1);

// ── Valkey keys (canonical, unprefixed — SSOT) ─────────────────────────────
//
// Canonical key strings/constructors. The deployment-time `VALKEY_KEY_PREFIX`
// is applied at the infrastructure boundary by `ValkeyAdapter` and by the
// pk-aware shims in `infrastructure::outbound::valkey_keys`.
//
// Application code imports these directly (domain-only) and passes the
// canonical key to `ValkeyPort`; the adapter prepends the prefix transparently.

// String constants used as raw keys.
pub const AGENT_INSTANCES_SET_KEY: &str = "veronex:agent:instances";
pub const INSTANCES_SET_KEY: &str = "veronex:instances";
pub const PUBSUB_JOB_EVENTS_KEY: &str = "veronex:pubsub:job_events";
pub const PUBSUB_CANCEL_PATTERN_KEY: &str = "veronex:pubsub:cancel:*";
pub const PUBSUB_CANCEL_PREFIX_KEY: &str = "veronex:pubsub:cancel:";
pub const VRAM_LEASES_SCAN_PATTERN_KEY: &str = "veronex:vram_leases:*";
pub const PROVIDERS_ONLINE_COUNTER_KEY: &str = "veronex:stats:providers:online";
pub const JOBS_PENDING_COUNTER_KEY: &str = "veronex:stats:jobs:pending";
pub const JOBS_RUNNING_COUNTER_KEY: &str = "veronex:stats:jobs:running";

// Job lifecycle.
pub fn job_owner_key(job_id: uuid::Uuid) -> String {
    format!("veronex:job:owner:{job_id}")
}
pub fn stream_tokens_key(job_id: uuid::Uuid) -> String {
    format!("veronex:stream:tokens:{job_id}")
}
pub fn pubsub_cancel_key(job_id: uuid::Uuid) -> String {
    format!("veronex:pubsub:cancel:{job_id}")
}

// Conversation cache.
pub fn conversation_record_key(conversation_id: uuid::Uuid) -> String {
    format!("veronex:conv:{conversation_id}")
}
pub fn conv_s3_cache_key(conv_id: uuid::Uuid) -> String {
    format!("conv_s3:{conv_id}")
}

// Instance / agent coordination.
pub fn heartbeat_key(instance_id: &str) -> String {
    format!("veronex:heartbeat:{instance_id}")
}
pub fn agent_heartbeat_key(hostname: &str) -> String {
    format!("veronex:agent:hb:{hostname}")
}
pub fn slot_leases_key(provider_id: uuid::Uuid, model: &str) -> String {
    format!("veronex:slot_leases:{provider_id}:{model}")
}

// Provider liveness / capacity.
pub fn provider_heartbeat_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:provider:hb:{provider_id}")
}
pub fn provider_capacity_state_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:provider:{provider_id}:capacity_state")
}
pub fn provider_models_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:models:{provider_id}")
}
pub fn hw_metrics_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:hw:{provider_id}")
}
pub fn server_node_metrics_key(server_id: uuid::Uuid) -> String {
    format!("veronex:server_metrics:{server_id}")
}
pub fn thermal_throttle_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:throttle:{provider_id}")
}

// VRAM pool.
pub fn vram_reserved_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:vram_reserved:{provider_id}")
}
pub fn vram_leases_key(provider_id: uuid::Uuid) -> String {
    format!("veronex:vram_leases:{provider_id}")
}

// Demand / scaleout / preload.
pub fn demand_key(model: &str) -> String {
    format!("veronex:demand:{model}")
}
pub fn preload_lock_key(model: &str, provider_id: uuid::Uuid) -> String {
    format!("veronex:preloading:{model}:{provider_id}")
}
pub fn scaleout_decision_key(model: &str) -> String {
    format!("veronex:scaleout:{model}")
}

// Auth / sessions.
pub fn revoked_jti_key(jti: uuid::Uuid) -> String {
    format!("veronex:revoked:{jti}")
}
pub fn password_reset_key(token: &str) -> String {
    format!("veronex:pwreset:{token}")
}
pub fn refresh_blocklist_key(hash: &str) -> String {
    format!("veronex:refresh_used:{hash}")
}
pub fn login_attempts_key(ip: &str) -> String {
    format!("veronex:login_attempts:{ip}")
}

// Rate limiting.
pub fn ratelimit_rpm_key(key_id: uuid::Uuid) -> String {
    format!("veronex:ratelimit:rpm:{key_id}")
}
pub fn ratelimit_tpm_key(key_id: uuid::Uuid, minute: i64) -> String {
    format!("veronex:ratelimit:tpm:{key_id}:{minute}")
}

// Gemini per-key counters.
pub fn gemini_rpm_key(provider_id: uuid::Uuid, model: &str, minute: i64) -> String {
    format!("veronex:gemini:rpm:{provider_id}:{model}:{minute}")
}
pub fn gemini_rpd_key(provider_id: uuid::Uuid, model: &str, date: &str) -> String {
    format!("veronex:gemini:rpd:{provider_id}:{model}:{date}")
}

// Ollama model context.
pub fn ollama_model_ctx_key(provider_id: uuid::Uuid, model_name: &str) -> String {
    format!("veronex:ollama:ctx:{provider_id}:{model_name}")
}

// Service health.
pub fn service_health_key(instance_id: &str) -> String {
    format!("veronex:svc:health:{instance_id}")
}

// MCP.
pub fn mcp_tool_key(server_id: uuid::Uuid) -> String {
    format!("veronex:mcp:tools:{server_id}")
}
pub fn mcp_tool_lock_key(server_id: uuid::Uuid) -> String {
    format!("veronex:mcp:tools:lock:{server_id}")
}
pub fn mcp_heartbeat_key(server_id: uuid::Uuid) -> String {
    format!("veronex:mcp:heartbeat:{server_id}")
}
pub fn mcp_key_acl_key(api_key_id: uuid::Uuid) -> String {
    format!("veronex:mcp:acl:{api_key_id}")
}
pub fn mcp_key_cap_points_key(api_key_id: uuid::Uuid) -> String {
    format!("veronex:mcp:cap:{api_key_id}")
}
pub fn mcp_key_top_k_key(api_key_id: uuid::Uuid) -> String {
    format!("veronex:mcp:topk:{api_key_id}")
}
pub fn mcp_result_key(tool_name: &str, args_hash: &str) -> String {
    format!("veronex:mcp:result:{tool_name}:{args_hash}")
}
pub fn mcp_tools_summary_key(server_id: uuid::Uuid) -> String {
    format!("veronex:mcp:tools_summary:{server_id}")
}

// ── Valkey key TTLs ────────────────────────────────────────────────────────

/// TTL (seconds) for the preload lock — covers a typical cold-load window.
pub const PRELOAD_LOCK_TTL_SECS: i64 = 180;

/// TTL (seconds) for the scale-out decision dedup lock.
pub const SCALEOUT_DECISION_TTL_SECS: i64 = 30;

/// TTL (seconds) for conversation caches in Valkey — shared between the
/// `veronex:conv:{conversation_id}` ConversationRecord cache (written by
/// runner / mcp bridge) and the `conv_s3:{conv_id}` full-turn-detail cache
/// (written by `fetch_conv_s3_cached` in conversation_handlers). Both must
/// agree so cache invalidation after S3 re-writes stays consistent.
pub const CONV_CACHE_TTL_SECS: i64 = 300;

/// TTL (seconds) for the per-API-key MCP caches (`mcp:acl`, `mcp:cap`,
/// `mcp:topk`). All three are invalidated explicitly on grant/revoke or key
/// update, so 60 s is just an upper bound on stale-after-restart windows.
pub const MCP_KEY_CACHE_TTL_SECS: i64 = 60;

/// TTL (seconds) for the MCP per-server tools summary cache
/// (`veronex:mcp:tools_summary:{server_id}`). Refreshed by the tool-discovery
/// background task; the 1-hour TTL is the safety net.
pub const MCP_TOOLS_SUMMARY_TTL_SECS: i64 = 3600;

/// TTL (seconds) for the per-(provider, model) Ollama context window cache
/// (`veronex:ollama:ctx:{provider_id}:{model_name}`). Written by the capacity
/// analyzer after each DB upsert; read on the inference hot path.
pub const OLLAMA_MODEL_CTX_TTL_SECS: i64 = 600;

/// TTL (seconds) for the per-provider model list cache
/// (`veronex:models:{provider_id}`). Mirrors the upstream Ollama `/api/tags`
/// freshness budget. Used by both `provider_handlers` (HTTP) and the capacity
/// analyzer (background sync), which is why it lives in domain rather than
/// the HTTP-layer constants.
pub const MODELS_CACHE_TTL_SECS: i64 = 3600;

/// TTL (seconds) for the per-job lease-attempts counter
/// (`veronex:queue:active:attempts:{job_id}`). 24 h gives ample window for
/// max-retry decisions while preventing unbounded counter accumulation.
pub const LEASE_ATTEMPTS_TTL_SECS: i64 = 86_400;

/// TTL (seconds) for the per-instance heartbeat key
/// (`veronex:heartbeat:{instance_id}`). 3× `REAPER_HEARTBEAT_INTERVAL` so a
/// single missed refresh doesn't trigger reaper takeover.
pub const INSTANCE_HEARTBEAT_TTL_SECS: i64 = 30;

/// TTL (seconds) for the password-reset token (`veronex:pwreset:{token}`).
/// 24 h matches the typical email-link expiry window.
pub const PASSWORD_RESET_TTL_SECS: i64 = 86_400;

/// Sliding window (seconds) for the per-IP login-attempts counter
/// (`veronex:login_attempts:{ip}`). The same value is returned to the client
/// in `Retry-After` when the rate limit trips, so both must stay aligned.
pub const LOGIN_ATTEMPTS_WINDOW_SECS: i64 = 300;

/// Default `Retry-After` (seconds) sent on 429 for generic rate-limit-exceeded
/// errors that surface from the domain layer. The TPM/RPM rate limiter
/// already returns a more specific value on its branch; this is the fallback.
pub const RATE_LIMIT_RETRY_AFTER_SECS: u64 = 60;

/// TTL (seconds) for the per-instance service-health HASH
/// (`veronex:svc:health:{instance_id}`). 2× the health-check pass interval
/// so a single missed pass does not auto-expire the key.
pub const SERVICE_HEALTH_TTL_SECS: i64 = 60;

// ── Thermal throttle ─────────────────────────────────────────────────────

/// Cooldown period (seconds) after hard thermal throttle is triggered.
/// During this window dispatch is suspended until temperature drops.
pub const THERMAL_HARD_COOLDOWN_SECS: i64 = 300;

/// TTL for the `veronex:thermal:{provider_id}` Valkey key.
/// Slightly longer than `THERMAL_HARD_COOLDOWN_SECS` to prevent stale key
/// from expiring before the cooldown window is checked.
pub const THERMAL_THROTTLE_KEY_TTL_SECS: i64 = 360;

// ── Gemini rate-limit TTLs ──────────────────────────────────────────────

/// TTL (seconds) for the per-minute Gemini RPM counter key.
pub const GEMINI_RPM_TTL_SECS: i64 = 120;

/// TTL (seconds) for the per-day Gemini RPD counter key (~25 hours).
pub const GEMINI_RPD_TTL_SECS: i64 = 90_000;

// ── Circuit breaker / reaper ─────────────────────────────────────────────

/// Cooldown before half-open probe after circuit opens.
pub const CIRCUIT_BREAKER_COOLDOWN: Duration = Duration::from_secs(60);

/// Sliding window size for P99 latency tracking per provider.
pub const CIRCUIT_BREAKER_LATENCY_WINDOW: usize = 100;

/// Minimum samples required before P99 latency can trigger soft degradation.
pub const CIRCUIT_BREAKER_LATENCY_MIN_SAMPLES: usize = 20;

/// P99 latency threshold (ms). When exceeded, circuit transitions to HalfOpen.
pub const CIRCUIT_BREAKER_P99_THRESHOLD_MS: u64 = 30_000;

/// Heartbeat interval for instance liveness (reaper).
pub const REAPER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

/// Interval between slot-lease reap passes.
pub const REAPER_SLOT_INTERVAL: Duration = Duration::from_secs(30);

/// Interval between orphaned-job queue reap passes.
pub const REAPER_QUEUE_INTERVAL: Duration = Duration::from_secs(60);

// ── MCP lifecycle phase flag ────────────────────────────────────────────────
//
// Feature flag for the Phase-1 lifecycle step in the runner. When enabled,
// `runner.process_job` invokes `provider.ensure_ready(model)` before
// `provider.stream_tokens(job)`. SDD: `.specs/veronex/history/inference-lifecycle-sod.md`.

/// Env var name for the lifecycle phase feature flag.
pub const MCP_LIFECYCLE_PHASE_FLAG_ENV: &str = "MCP_LIFECYCLE_PHASE";

/// Default value when the env var is absent or unparseable. Default `false`
/// preserves pre-Tier-C behaviour (implicit auto-load via `stream_tokens`)
/// until live verification on dev.
pub const MCP_LIFECYCLE_PHASE_DEFAULT: bool = false;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── Scoring helper (pure function, extracted for testability) ─────────

    /// Compute ZSET enqueue score. Pure function.
    fn enqueue_score(now_ms: u64, tier_bonus: u64) -> f64 {
        now_ms.saturating_sub(tier_bonus) as f64
    }

    /// Compute dispatcher final_score. Pure function.
    fn final_score(zset_score: f64, locality_bonus: f64, wait_ms: f64, perf_factor: f64) -> f64 {
        let age_bonus = wait_ms * 0.25 * perf_factor;
        zset_score - locality_bonus - age_bonus
    }

    // ── Property-based tests (proptest) ──────────────────────────────────
    //
    // Valkey key-format guards live alongside their constructors in
    // `infrastructure::outbound::valkey_keys` (single source of truth for
    // both the function and the format invariant).

    proptest! {
        /// Tier ordering invariant: for any timestamp,
        /// paid_score < standard_score < test_score (lower = higher priority).
        #[test]
        fn tier_ordering_invariant(now_ms in TIER_BONUS_PAID..=u64::MAX) {
            let paid = enqueue_score(now_ms, TIER_BONUS_PAID);
            let standard = enqueue_score(now_ms, TIER_BONUS_STANDARD);
            let test = enqueue_score(now_ms, TIER_BONUS_TEST);

            prop_assert!(paid < standard, "paid ({paid}) must < standard ({standard})");
            prop_assert!(standard < test, "standard ({standard}) must < test ({test})");
        }

        /// Paid job submitted up to 199s after standard still wins.
        /// At exactly 200s gap, they tie (boundary).
        #[test]
        fn paid_dominates_within_tier_window(
            t0 in TIER_BONUS_PAID..=(u64::MAX - 200_000),
            gap_ms in 0_u64..200_000,
        ) {
            let standard = enqueue_score(t0, TIER_BONUS_STANDARD);
            let paid = enqueue_score(t0 + gap_ms, TIER_BONUS_PAID);
            prop_assert!(paid <= standard,
                "paid (gap={gap_ms}ms) score {paid} must <= standard {standard}");
        }

        /// Age bonus monotonicity: longer wait → larger age_bonus → lower final_score.
        #[test]
        fn age_bonus_monotonic(
            zset_score in -1e15_f64..1e15,
            wait1 in 0.0_f64..1e9,
            extra in 0.001_f64..1e9,
            pf in 0.01_f64..=1.0,
        ) {
            let wait2 = wait1 + extra;
            let fs1 = final_score(zset_score, 0.0, wait1, pf);
            let fs2 = final_score(zset_score, 0.0, wait2, pf);
            prop_assert!(fs2 < fs1,
                "longer wait ({wait2}) should yield lower final_score ({fs2}) than ({wait1}) → ({fs1})");
        }

        /// Locality boost: loaded model always gets lower final_score (higher priority).
        #[test]
        fn locality_boost_always_helps(
            zset_score in -1e15_f64..1e15,
            wait_ms in 0.0_f64..1e9,
            pf in 0.0_f64..=1.0,
        ) {
            let with_locality = final_score(zset_score, LOCALITY_BONUS_MS, wait_ms, pf);
            let without = final_score(zset_score, 0.0, wait_ms, pf);
            prop_assert!(with_locality < without,
                "locality ({with_locality}) must < no-locality ({without})");
        }

        /// perf_factor scaling: higher perf_factor → larger age_bonus → lower final_score.
        #[test]
        fn perf_factor_amplifies_age(
            zset_score in -1e15_f64..1e15,
            wait_ms in 1.0_f64..1e9,
            pf_low in 0.0_f64..0.5,
            pf_delta in 0.01_f64..0.5,
        ) {
            let pf_high = pf_low + pf_delta;
            let fs_low = final_score(zset_score, 0.0, wait_ms, pf_low);
            let fs_high = final_score(zset_score, 0.0, wait_ms, pf_high);
            prop_assert!(fs_high < fs_low,
                "higher perf_factor ({pf_high}) → lower final_score ({fs_high}) vs ({fs_low})");
        }

        /// Starvation guarantee: after TIER_EXPIRE_SECS, age_bonus can overcome
        /// the tier gap (200,000ms) between paid and standard at perf_factor=1.0.
        #[test]
        fn starvation_breaks_within_tier_expire(
            t0 in TIER_BONUS_PAID..=(u64::MAX - 300_000),
        ) {
            let standard_score = enqueue_score(t0, TIER_BONUS_STANDARD);
            let t_new = t0 + TIER_EXPIRE_SECS * 1000;
            let paid_score = enqueue_score(t_new, TIER_BONUS_PAID);

            // Standard has been waiting TIER_EXPIRE_SECS, paid just arrived
            let wait_standard = (TIER_EXPIRE_SECS * 1000) as f64;
            let wait_paid = 0.0;

            let fs_standard = final_score(standard_score, 0.0, wait_standard, 1.0);
            let fs_paid = final_score(paid_score, 0.0, wait_paid, 1.0);

            prop_assert!(fs_standard < fs_paid,
                "standard waiting {}s must beat fresh paid: standard={fs_standard} vs paid={fs_paid}",
                TIER_EXPIRE_SECS);
        }
    }
}
