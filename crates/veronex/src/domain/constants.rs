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

/// Prefix prepended to every generated API key plaintext (e.g. `iq_<base62>`).
pub const API_KEY_PREFIX: &str = "iq_";

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

/// Scoring bonus (MB) for models already loaded in VRAM (locality preference).
pub const MODEL_LOCALITY_BONUS_MB: i64 = 100_000;

// ── HTTP request timeouts ──────────────────────────────────────────────────

/// Timeout for inference requests to Ollama/Gemini providers (5 min).
pub const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout for Ollama API metadata calls (/api/show, /api/tags, /api/ps).
pub const OLLAMA_METADATA_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for Ollama health check (/api/version).
pub const OLLAMA_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for Gemini health check (lightweight models list).
pub const GEMINI_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for veronex-agent metrics fetch.
pub const AGENT_METRICS_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for LLM single-model analysis call.
pub const LLM_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for LLM batch analysis call (all models).
pub const LLM_BATCH_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(60);

/// Timeout for node-exporter metrics fetch.
pub const NODE_EXPORTER_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for job cancellation in CancelGuard.
pub const CANCEL_TIMEOUT: Duration = Duration::from_secs(5);

// ── Cache TTL ──────────────────────────────────────────────────────────────

/// TTL for OllamaModel provider-for-model lookup cache (hot path).
pub const OLLAMA_MODEL_CACHE_TTL: Duration = Duration::from_secs(10);

/// TTL for provider-model-selection enabled list cache.
pub const MODEL_SELECTION_CACHE_TTL: Duration = Duration::from_secs(30);

/// TTL for the CachingProviderRegistry in-memory snapshot.
pub const PROVIDER_REGISTRY_CACHE_TTL: Duration = Duration::from_secs(5);

// ── Sync / sweep intervals ────────────────────────────────────────────────

/// Base tick interval for the capacity analyzer sync loop.
pub const SYNC_LOOP_BASE_TICK: Duration = Duration::from_secs(30);

/// Interval between pending-job sweep passes (reaper).
pub const PENDING_JOB_SWEEP_INTERVAL: Duration = Duration::from_secs(300);

// ── Valkey key constructors (used by application layer) ─────────────────

/// Job ownership key — tracks which instance owns a running job.
pub fn job_owner_key(job_id: uuid::Uuid) -> String {
    format!("veronex:job:owner:{job_id}")
}

/// TPM (tokens per minute) counter key for an API key at a given minute epoch.
pub fn ratelimit_tpm_key(key_id: uuid::Uuid, minute: i64) -> String {
    format!("veronex:ratelimit:tpm:{key_id}:{minute}")
}

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
