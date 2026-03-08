use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::domain::entities::ApiKey;
use crate::infrastructure::inbound::http::state::AppState;
use crate::infrastructure::outbound::valkey_keys;

/// 1-minute sliding window for RPM.
const RPM_WINDOW_MS: f64 = 60_000.0;

// ── Middleware ─────────────────────────────────────────────────────────────────

/// Sliding-window rate limiting (RPM + TPM) via Valkey sorted sets / counters.
///
/// Reads the `ApiKey` injected by the auth middleware.
/// * `rate_limit_rpm == 0` → unlimited RPM
/// * `rate_limit_tpm == 0` → unlimited TPM
/// * Valkey unavailable → fail-closed (503 Service Unavailable)
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

    // Valkey unavailable → fail-closed (503).
    // Security rationale: fail-open would allow unlimited requests during outage.
    // Auth endpoints use fail-open (login attempts) because blocking logins is worse.
    let Some(ref pool) = state.valkey_pool else {
        tracing::warn!(key_id = %api_key.id, "rate limiter: Valkey unavailable, fail-closed");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "rate limiting service unavailable"})),
        )
            .into_response();
    };

    // ── RPM check ────────────────────────────────────────────────────
    if api_key.rate_limit_rpm > 0 {
        let key = valkey_keys::ratelimit_rpm(api_key.id);
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
                tracing::error!(key_id = %api_key.id, "RPM check error: {e}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "rate limiting service unavailable"})),
                )
                    .into_response();
            }
            Ok(true) => {}
        }
    }

    // ── TPM check ────────────────────────────────────────────────────
    if api_key.rate_limit_tpm > 0 {
        let minute = current_minute();
        let key = valkey_keys::ratelimit_tpm(api_key.id, minute);

        match check_tpm(pool, &key, api_key.rate_limit_tpm as u64).await {
            Ok(false) => {
                tracing::debug!(key_id = %api_key.id, "TPM limit exceeded");
                return rate_limit_response("tpm", api_key.rate_limit_tpm as u64, seconds_until_next_minute());
            }
            Err(e) => {
                tracing::error!(key_id = %api_key.id, "TPM check error: {e}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "rate limiting service unavailable"})),
                )
                    .into_response();
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
///
/// Note: TTL is 62s (not 60s) — the 2s buffer accounts for Valkey clock skew
/// relative to the application, preventing premature key eviction during the
/// tail end of a sliding window.
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
    pool: &fred::clients::Pool,
    key: &str,
    now_ms: f64,
    limit: u64,
    member: &str,
) -> anyhow::Result<bool> {
    use fred::interfaces::LuaInterface as _;

    let window_start = now_ms - RPM_WINDOW_MS;

    // pool.next() returns Arc<Client> which implements LuaInterface.
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

// ── TPM: per-minute counter with atomic reservation ──────────────────────────

use super::super::constants::TPM_ESTIMATED_TOKENS;

/// Lua script: atomic check-and-reserve for TPM.
///
/// KEYS[1] = tpm counter key
/// ARGV[1] = limit  ARGV[2] = estimated_tokens
///
/// Returns the counter value BEFORE increment. If over limit, does NOT increment.
const TPM_RESERVE_SCRIPT: &str = r#"
local current = tonumber(redis.call('GET', KEYS[1]) or '0')
if current >= tonumber(ARGV[1]) then
  return -1
end
redis.call('INCRBY', KEYS[1], tonumber(ARGV[2]))
if redis.call('TTL', KEYS[1]) < 0 then
  redis.call('EXPIRE', KEYS[1], 120)
end
return current
"#;

/// Atomically check TPM limit and reserve estimated tokens in a single Lua eval.
/// Returns `true` when the request is within the limit.
async fn check_tpm(
    pool: &fred::clients::Pool,
    key: &str,
    limit: u64,
) -> anyhow::Result<bool> {
    use fred::interfaces::LuaInterface as _;

    let result: i64 = pool
        .next()
        .eval(
            TPM_RESERVE_SCRIPT,
            vec![key.to_string()],
            vec![limit.to_string(), TPM_ESTIMATED_TOKENS.to_string()],
        )
        .await?;

    // -1 means limit exceeded (no reservation made)
    Ok(result >= 0)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::enums::{KeyTier, KeyType};

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
            key_type: KeyType::Standard,
            tier: KeyTier::Paid,
        };
        assert_eq!(key.rate_limit_rpm, 0);
        assert_eq!(key.rate_limit_tpm, 0);
    }

    #[test]
    fn rate_limit_key_format() {
        let id = uuid::Uuid::now_v7();
        let rpm_key = valkey_keys::ratelimit_rpm(id);
        let tpm_key = valkey_keys::ratelimit_tpm(id, current_minute());
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
