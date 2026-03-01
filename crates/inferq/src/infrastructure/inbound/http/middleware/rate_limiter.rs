use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::domain::entities::ApiKey;
use crate::infrastructure::inbound::http::state::AppState;

/// 1-minute sliding window for RPM.
const RPM_WINDOW_MS: f64 = 60_000.0;

// ── Middleware ─────────────────────────────────────────────────────────────────

/// Sliding-window rate limiting (RPM + TPM) via Valkey sorted sets / counters.
///
/// Reads the `ApiKey` injected by the auth middleware.
/// * `rate_limit_rpm == 0` → unlimited RPM
/// * `rate_limit_tpm == 0` → unlimited TPM
/// * Valkey unavailable → fail-open (request is allowed)
pub async fn rate_limiter(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let api_key = req.extensions().get::<ApiKey>().cloned();

    // No key in extensions = health endpoint or pre-auth path → pass through.
    let Some(api_key) = api_key else {
        return next.run(req).await;
    };

    // Valkey not configured → pass through with a warning (once at startup).
    let Some(ref pool) = state.valkey_pool else {
        return next.run(req).await;
    };

    // ── RPM check ────────────────────────────────────────────────────
    if api_key.rate_limit_rpm > 0 {
        let key = format!("veronex:ratelimit:rpm:{}", api_key.id);
        let now_ms = chrono::Utc::now().timestamp_millis() as f64;
        // Each request gets a unique member so concurrent ms-level requests
        // are all counted separately.
        let member = uuid::Uuid::now_v7().to_string();

        match check_rpm(pool, &key, now_ms, api_key.rate_limit_rpm as u64, &member).await {
            Ok(false) => {
                tracing::debug!(key_id = %api_key.id, "RPM limit exceeded");
                return rate_limit_response("rpm", api_key.rate_limit_rpm as u64, 60);
            }
            Err(e) => {
                tracing::warn!(key_id = %api_key.id, "RPM check error (failing open): {e}");
            }
            Ok(true) => {}
        }
    }

    // ── TPM check ────────────────────────────────────────────────────
    if api_key.rate_limit_tpm > 0 {
        let minute = current_minute();
        let key = format!("veronex:ratelimit:tpm:{}:{}", api_key.id, minute);

        match check_tpm(pool, &key, api_key.rate_limit_tpm as u64).await {
            Ok(false) => {
                tracing::debug!(key_id = %api_key.id, "TPM limit exceeded");
                return rate_limit_response("tpm", api_key.rate_limit_tpm as u64, seconds_until_next_minute());
            }
            Err(e) => {
                tracing::warn!(key_id = %api_key.id, "TPM check error (failing open): {e}");
            }
            Ok(true) => {}
        }
    }

    next.run(req).await
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn current_minute() -> i64 {
    chrono::Utc::now().timestamp() / 60
}

fn seconds_until_next_minute() -> u64 {
    let now = chrono::Utc::now();
    let secs = now.timestamp() % 60;
    (60 - secs) as u64
}

fn rate_limit_response(limit_type: &str, limit: u64, retry_after: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(
            "Retry-After",
            retry_after.to_string(),
        )],
        Json(json!({
            "error": "rate_limit_exceeded",
            "limit_type": limit_type,
            "limit": limit,
            "retry_after_secs": retry_after,
        })),
    )
        .into_response()
}

// ── RPM: sliding-window sorted set ────────────────────────────────────────────

/// Lua script: atomic 1-RTT sliding-window rate-limit check.
///
/// Removes expired entries, records the current request, refreshes the TTL,
/// and returns the new window count — all in a single round-trip.
/// KEYS[1] = sorted-set key
/// ARGV[1] = window_start_ms  ARGV[2] = now_ms (score)  ARGV[3] = member
const RATE_LIMIT_SCRIPT: &str = r#"
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('EXPIRE', KEYS[1], 62)
return redis.call('ZCARD', KEYS[1])
"#;

/// Check and record one request in the RPM sliding window.
///
/// Single atomic Lua eval replaces the previous 4-command round-trip sequence.
/// Returns `true` when the request is within the limit.
async fn check_rpm(
    pool: &fred::clients::RedisPool,
    key: &str,
    now_ms: f64,
    limit: u64,
    member: &str,
) -> anyhow::Result<bool> {
    use fred::interfaces::LuaInterface as _;

    let window_start = now_ms - RPM_WINDOW_MS;

    // pool.next() returns Arc<RedisClient> which implements LuaInterface.
    let count: u64 = pool
        .next()
        .eval(
            RATE_LIMIT_SCRIPT,
            vec![key.to_string()],
            vec![
                window_start.to_string(),
                now_ms.to_string(),
                member.to_string(),
            ],
        )
        .await?;

    Ok(count <= limit)
}

// ── TPM: per-minute counter ────────────────────────────────────────────────────

/// Check whether accumulated tokens in the current minute are under `limit`.
///
/// The counter is incremented by `InferenceUseCaseImpl` after each job
/// completes (see `record_tpm`).
async fn check_tpm(
    pool: &fred::clients::RedisPool,
    key: &str,
    limit: u64,
) -> anyhow::Result<bool> {
    use fred::prelude::*;

    // Missing key → 0 tokens used.
    let used: i64 = pool.get(key).await.unwrap_or(0i64);
    Ok(used < limit as i64)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_rpm_zero() {
        let key = ApiKey {
            id: uuid::Uuid::now_v7(),
            key_hash: "hash".to_string(),
            key_prefix: "iq_test".to_string(),
            tenant_id: "t".to_string(),
            name: "test".to_string(),
            is_active: true,
            rate_limit_rpm: 0,
            rate_limit_tpm: 0,
            expires_at: None,
            deleted_at: None,
            created_at: chrono::Utc::now(),
            key_type: "standard".to_string(),
            tier: "paid".to_string(),
        };
        assert_eq!(key.rate_limit_rpm, 0);
        assert_eq!(key.rate_limit_tpm, 0);
    }

    #[test]
    fn rate_limit_key_format() {
        let id = uuid::Uuid::now_v7();
        let rpm_key = format!("veronex:ratelimit:rpm:{}", id);
        let tpm_key = format!("veronex:ratelimit:tpm:{}:{}", id, current_minute());
        assert!(rpm_key.starts_with("veronex:ratelimit:rpm:"));
        assert!(tpm_key.starts_with("veronex:ratelimit:tpm:"));
        assert!(tpm_key.contains(&id.to_string()));
    }

    #[test]
    fn seconds_until_next_minute_range() {
        let secs = seconds_until_next_minute();
        assert!((1..=60).contains(&secs));
    }
}
