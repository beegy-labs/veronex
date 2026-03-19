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

/// Scoring bonus (MB) for models already loaded in VRAM (locality preference).
pub const MODEL_LOCALITY_BONUS_MB: i64 = 100_000;

// ── ZSET queue (Phase 3) ──────────────────────────────────────────────────

/// Unified priority queue (ZSET). Lower score = higher priority.
pub const QUEUE_ZSET: &str = "veronex:queue:zset";

/// Side hash: job_id → enqueue_at_ms (for promote_overdue & age_bonus).
pub const QUEUE_ENQUEUE_AT: &str = "veronex:queue:enqueue_at";

/// Side hash: job_id → model (for demand_resync).
pub const QUEUE_MODEL_MAP: &str = "veronex:queue:model";

/// Per-model demand counter prefix. Full key: `veronex:demand:{model}`.
pub const DEMAND_PREFIX: &str = "veronex:demand:";

/// Build the demand counter key for a model.
pub fn demand_key(model: &str) -> String {
    format!("{DEMAND_PREFIX}{model}")
}

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

/// Timeout for Whisper ASR health check (`GET /`).
pub const WHISPER_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for Whisper ASR transcription requests (large audio files).
pub const WHISPER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

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

/// Interval between health checker passes (provider liveness probes).
pub const HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

/// Interval between pending-job sweep passes (reaper).
pub const PENDING_JOB_SWEEP_INTERVAL: Duration = Duration::from_secs(300);

/// Tick interval for the real-time FlowStats broadcast ticker.
pub const STATS_TICK_INTERVAL: Duration = Duration::from_secs(1);

// ── Valkey key constructors (used by application layer) ─────────────────

/// Job ownership key — tracks which instance owns a running job.
pub fn job_owner_key(job_id: uuid::Uuid) -> String {
    format!("veronex:job:owner:{job_id}")
}

/// TPM (tokens per minute) counter key for an API key at a given minute epoch.
pub fn ratelimit_tpm_key(key_id: uuid::Uuid, minute: i64) -> String {
    format!("veronex:ratelimit:tpm:{key_id}:{minute}")
}

/// Lock key preventing duplicate preload requests for a (model, provider) pair.
pub fn preload_lock_key(model: &str, provider_id: uuid::Uuid) -> String {
    format!("veronex:preloading:{model}:{provider_id}")
}

/// Scale-out decision dedup key for a model.
pub fn scaleout_decision_key(model: &str) -> String {
    format!("veronex:scaleout:{model}")
}

// ── Thermal throttle ─────────────────────────────────────────────────────

/// Cooldown period (seconds) after hard thermal throttle is triggered.
/// During this window dispatch is suspended until temperature drops.
pub const THERMAL_HARD_COOLDOWN_SECS: i64 = 300;

/// TTL for the `veronex:thermal:{provider_id}` Valkey key.
/// Slightly longer than `THERMAL_HARD_COOLDOWN_SECS` to prevent stale key
/// from expiring before the cooldown window is checked.
pub const THERMAL_THROTTLE_KEY_TTL_SECS: i64 = 360;

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

    // ── Fixed assertions (structural invariants) ─────────────────────────

    #[test]
    fn emergency_equals_paid_bonus() {
        assert_eq!(EMERGENCY_BONUS_MS, TIER_BONUS_PAID);
    }

    #[test]
    fn queue_limits_reasonable() {
        assert_eq!(MAX_QUEUE_SIZE, 10_000);
        assert_eq!(MAX_QUEUE_PER_MODEL, 2_000);
        assert!(MAX_QUEUE_PER_MODEL < MAX_QUEUE_SIZE);
    }

    #[test]
    fn demand_key_format() {
        assert_eq!(demand_key("llama3:70b"), "veronex:demand:llama3:70b");
    }

    #[test]
    fn job_owner_key_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            job_owner_key(id),
            "veronex:job:owner:00000000-0000-0000-0000-000000000000",
        );
    }

    #[test]
    fn ratelimit_tpm_key_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            ratelimit_tpm_key(id, 1_710_600_000),
            "veronex:ratelimit:tpm:00000000-0000-0000-0000-000000000000:1710600000",
        );
    }

    #[test]
    fn preload_lock_key_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            preload_lock_key("qwen3:8b", id),
            "veronex:preloading:qwen3:8b:00000000-0000-0000-0000-000000000000",
        );
    }

    #[test]
    fn scaleout_decision_key_format() {
        assert_eq!(
            scaleout_decision_key("llama3:70b"),
            "veronex:scaleout:llama3:70b",
        );
    }

    #[test]
    fn adaptive_k_bounds() {
        assert!(ZSET_PEEK_K >= 20);
        assert!(ZSET_PEEK_K_MAX <= 100);
        assert!(ZSET_PEEK_K <= ZSET_PEEK_K_MAX);
    }

    // ── Property-based tests (proptest) ──────────────────────────────────

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
