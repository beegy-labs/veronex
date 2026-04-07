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

/// Hard timeout for SSE streams (3 minutes).
///
/// Force-closes zombie SSE connections that neither complete nor disconnect.
/// This is a safety net — normal inference jobs complete well within this window.
/// 180 s is sufficient for typical LLM inference; long-running jobs should use
/// polling instead of keeping an SSE connection open.
pub const SSE_TIMEOUT: Duration = Duration::from_secs(180);

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
/// use `SSE_TIMEOUT` (3 min) instead.
pub const JWT_ROUTER_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for the inference/API router (240 s).
///
/// Covers non-streaming inference requests (synchronous chat completions,
/// embeddings, Ollama passthrough). Set higher than `SSE_TIMEOUT` (180 s)
/// so SSE streams are killed by their own inner timeout first; this timeout
/// only fires on hung non-streaming requests.
pub const INFERENCE_ROUTER_TIMEOUT: Duration = Duration::from_secs(240);

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
