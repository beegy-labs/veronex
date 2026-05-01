//! HTTP-layer constants.
//!
//! SSE and caching constants specific to the HTTP infrastructure layer.
//! Application-layer constants (job lifecycle, TPM, routing) live in
//! `crate::domain::constants` and are re-exported here for convenience.

use std::time::Duration;

// ── Re-exports from domain::constants ───────────────────────────────────────
// Allows existing `use super::constants::TPM_ESTIMATED_TOKENS;` in HTTP modules
// to keep working without changing every import site.
pub use crate::domain::constants::{
    GEMINI_TIER_FREE, INITIAL_TOKEN_CAPACITY, JOB_CLEANUP_DELAY, JOB_OWNER_TTL_SECS,
    KEY_TIER_PAID, MAX_CHAT_MESSAGES, MAX_TOKENS_CEILING, NO_PROVIDER_BACKOFF,
    OLLAMA_HEALTH_CHECK_TIMEOUT, OWNERSHIP_LOST_CLEANUP_DELAY, OWNER_REFRESH_INTERVAL,
    PENDING_JOB_SWEEP_INTERVAL, PROVIDER_REGISTRY_CACHE_TTL, QUEUE_ERROR_BACKOFF,
    QUEUE_POLL_INTERVAL, SYNC_LOOP_BASE_TICK, TPM_ESTIMATED_TOKENS,
};

// ── SSE ──────────────────────────────────────────────────────────────────────

/// SSE keep-alive ping interval.
///
/// Prevents reverse proxies (nginx, Cloudflare) from closing idle connections.
/// All SSE endpoints must use this value for consistency.
pub const SSE_KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Maximum concurrent SSE connections across the entire instance.
///
/// Prevents resource exhaustion (file descriptors, Valkey connections) from
/// too many open SSE streams. Exceeding this returns HTTP 429.
pub const SSE_MAX_CONNECTIONS: u32 = 100;

/// Hard timeout for SSE streams (28 minutes 20 seconds).
///
/// Force-closes zombie SSE connections that neither complete nor disconnect.
/// Sized to cover the longest MCP-routed request the bridge can produce:
///   - phase-aware lifecycle (200k cold load worst-case ≈ 600 s)
///   - up to `MAX_ROUNDS=5` rounds, each capped by `ROUND_TOTAL_TIMEOUT=1500 s`
///     (per `docs/llm/inference/mcp.md`)
///   - one optional synthesis round (S24) on top, same per-round budget
///
/// Held strictly below the upstream Cilium gateway HTTPRoute timeout
/// (`timeouts.request=1800 s`, see `.add/domain-integration.md`) so the SSE
/// wrapper closes the stream cleanly before the gateway 504s the request.
/// Locking the relationship as `SSE_TIMEOUT < gateway.timeouts.request` is
/// required: if SSE_TIMEOUT > gateway timeout, the client sees an opaque
/// 504 instead of a clean `event: error data: stream timeout`.
pub const SSE_TIMEOUT: Duration = Duration::from_secs(1700);

// ── Input validation ──────────────────────────────────────────────────────────

/// Maximum prompt length in bytes (1 MB).
///
/// Prevents memory exhaustion from oversized payloads that bypass Axum's
/// body-size limit (which applies to the entire JSON envelope, not individual fields).
pub const MAX_PROMPT_BYTES: usize = 1_000_000;

/// Maximum model name length in bytes.
///
/// Model names are short identifiers (e.g. "llama3.2:latest"). 256 bytes is
/// generous enough for any realistic model tag while blocking abuse.
pub const MAX_MODEL_NAME_BYTES: usize = 256;

/// Maximum longest edge (px) for server-side image compression.
///
/// Oversized images are resized to fit this dimension (aspect-ratio preserved)
/// and re-encoded as WebP before forwarding to Ollama and storing in S3.
/// 1024px covers the sweet spot for most vision models:
/// - Qwen3-VL / Qwen2.5-VL optimal range: 480–2560px
/// - Gemma 3 internal: 896px
/// - LLaVA internal: 336px
pub const IMAGE_COMPRESS_MAX_EDGE: u32 = 1024;

// ── Provider type identifiers ────────────────────────────────────────────────

/// Provider type string for Ollama providers (used in submit calls and routing).
pub const PROVIDER_OLLAMA: &str = "ollama";

/// Provider type string for Gemini providers.
pub const PROVIDER_GEMINI: &str = "gemini";

// ── Auth cookie TTLs ────────────────────────────────────────────────────────

/// Access token cookie Max-Age (seconds). Must match JWT expiry (1 hour).
pub const ACCESS_TOKEN_MAX_AGE: u32 = 3600;

/// Refresh token cookie Max-Age (seconds). Must match session expiry (7 days).
pub const REFRESH_TOKEN_MAX_AGE: u32 = 604800;

// ── Valkey / caching ─────────────────────────────────────────────────────────

/// Valkey TTL (seconds) for the per-provider model list cache.
///
/// After this period a cache miss triggers a live fetch from the provider.
pub const MODELS_CACHE_TTL: i64 = 3600;

// ── Timeout constants ────────────────────────────────────────────────────────

/// Timeout for the JWT-authenticated admin router (30 s).
///
/// Applies to all non-inference, non-streaming endpoints. Inference routes
/// use `SSE_TIMEOUT` (≈28 min) for streaming and `INFERENCE_ROUTER_TIMEOUT`
/// (≈30 min) for non-streaming.
pub const JWT_ROUTER_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for the inference/API router (1750 s ≈ 29 min).
///
/// Covers non-streaming inference requests (synchronous chat completions,
/// embeddings, Ollama passthrough). Set strictly higher than `SSE_TIMEOUT`
/// (1700 s) so SSE streams are killed by their own inner timeout first;
/// this only fires on hung non-streaming requests. Held under Cilium
/// gateway `timeouts.request=1800 s`.
pub const INFERENCE_ROUTER_TIMEOUT: Duration = Duration::from_secs(1750);

/// Timeout for the dashboard queue-depth Valkey fetch (3 s).
///
/// Best-effort — dashboard degrades gracefully on Valkey timeout.
pub const DASHBOARD_QUEUE_DEPTH_TIMEOUT: Duration = Duration::from_secs(3);

/// Timeout for the dashboard stats aggregate fetch (10 s).
///
/// Covers parallel DB + analytics queries in `/v1/dashboard/stats`.
pub const DASHBOARD_STATS_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for an outbound vision-analysis HTTP call (120 s).
///
/// Vision models may take longer than text models on first load.
pub const VISION_HTTP_TIMEOUT: Duration = Duration::from_secs(120);

// ── Body limits ─────────────────────────────────────────────────────────────

/// Default JSON body limit applied at the global router level (1 MB).
///
/// Rejects oversized bodies before deserialization.  Image-capable endpoints
/// override this per-route with `IMAGE_BODY_LIMIT`.
pub const JSON_BODY_LIMIT: usize = 1024 * 1024;

/// Body limit for image-capable endpoints (20 MB).
pub const IMAGE_BODY_LIMIT: usize = 20 * 1024 * 1024;

// ── Per-key concurrency ──────────────────────────────────────────────────────

/// Maximum concurrent in-flight requests per API key.
///
/// Prevents Slowloris-style attacks where a single key holds many long-running
/// connections, exhausting threads/file descriptors across the cluster.
pub const MAX_KEY_CONCURRENT: u32 = 10;

// ── Pagination ──────────────────────────────────────────────────────────────

/// Hard upper bound on page numbers accepted from clients.
///
/// Prevents (page - 1) * limit from overflowing i64 at extreme values and
/// protects against slow-deep-offset queries (`OFFSET 10_000_000` scans every
/// row up to that point even with an index).
pub const MAX_PAGE: i64 = 10_000;

// ── Error messages ──────────────────────────────────────────────────────────

pub const ERR_DATABASE: &str = "database error";
pub const ERR_MODEL_INVALID: &str = "model name invalid or too long";
pub const ERR_PROMPT_TOO_LARGE: &str = "content exceeds maximum length of 1MB";

// ── Timeout invariants ──────────────────────────────────────────────────────
//
// SSE_TIMEOUT and INFERENCE_ROUTER_TIMEOUT must stay coordinated with the
// Cilium gateway HTTPRoute timeout (1800s) and the bridge ROUND_TOTAL_TIMEOUT
// (1500s, in `bridge.rs`). A regression in any of these would cause the user
// to see an opaque 504 mid-stream instead of a clean SSE error.

#[cfg(test)]
mod timeout_invariants {
    use super::*;

    /// Cilium HTTPRoute `timeouts.request` cap from platform-gitops
    /// `clusters/home/values/cilium-gateway-values.yaml`.
    const CILIUM_GATEWAY_TIMEOUT: Duration = Duration::from_secs(1800);

    /// Worst-case bridge round budget. Mirrors `bridge::ROUND_TOTAL_TIMEOUT`.
    /// Pinned here independently so a regression in either constant is caught.
    const BRIDGE_ROUND_TOTAL_TIMEOUT: Duration = Duration::from_secs(1500);

    #[test]
    fn sse_timeout_under_cilium_gateway() {
        assert!(
            SSE_TIMEOUT < CILIUM_GATEWAY_TIMEOUT,
            "SSE_TIMEOUT ({:?}) must be < Cilium gateway timeout ({:?}) — \
             otherwise the client sees an opaque 504 instead of a clean SSE error",
            SSE_TIMEOUT, CILIUM_GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn sse_timeout_covers_one_full_bridge_round() {
        assert!(
            SSE_TIMEOUT >= BRIDGE_ROUND_TOTAL_TIMEOUT,
            "SSE_TIMEOUT ({:?}) must cover at least ROUND_TOTAL_TIMEOUT ({:?}) \
             so a single legitimate slow MCP round doesn't trip the SSE wrapper",
            SSE_TIMEOUT, BRIDGE_ROUND_TOTAL_TIMEOUT
        );
    }

    #[test]
    fn inference_router_timeout_under_cilium_gateway() {
        assert!(
            INFERENCE_ROUTER_TIMEOUT < CILIUM_GATEWAY_TIMEOUT,
            "INFERENCE_ROUTER_TIMEOUT ({:?}) must be < Cilium gateway timeout ({:?})",
            INFERENCE_ROUTER_TIMEOUT, CILIUM_GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn inference_router_timeout_above_sse_timeout() {
        assert!(
            INFERENCE_ROUTER_TIMEOUT > SSE_TIMEOUT,
            "INFERENCE_ROUTER_TIMEOUT ({:?}) must be > SSE_TIMEOUT ({:?}) — \
             SSE wrapper is the inner timeout for streaming, router is the outer fallback",
            INFERENCE_ROUTER_TIMEOUT, SSE_TIMEOUT
        );
    }
}
