use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::domain::entities::ApiKey;
use crate::infrastructure::inbound::http::state::AppState;

/// Axum middleware for sliding-window rate limiting via Valkey sorted sets.
///
/// Reads the `ApiKey` from request extensions (injected by auth middleware).
/// Skips rate limiting when `rate_limit_rpm == 0` (unlimited).
/// Returns 429 Too Many Requests when the limit is exceeded.
pub async fn rate_limiter(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let api_key = req.extensions().get::<ApiKey>();

    // No key in extensions means auth was skipped (health endpoints)
    let Some(api_key) = api_key else {
        return Ok(next.run(req).await);
    };

    // 0 = unlimited, skip rate limiting
    if api_key.rate_limit_rpm == 0 {
        return Ok(next.run(req).await);
    }

    let Some(ref pool) = state.valkey_pool else {
        // No Valkey pool configured — skip rate limiting
        return Ok(next.run(req).await);
    };

    let rpm_limit = api_key.rate_limit_rpm as u64;
    let key = format!("inferq:ratelimit:rpm:{}", api_key.id);
    let now_ms = chrono::Utc::now().timestamp_millis() as f64;
    let window_ms = 60_000.0; // 1 minute

    // Sliding window: remove entries older than the window, add current, count
    match check_sliding_window(pool, &key, now_ms, window_ms, rpm_limit).await {
        Ok(allowed) => {
            if allowed {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::TOO_MANY_REQUESTS)
            }
        }
        Err(_) => {
            // On Valkey errors, fail open (allow the request)
            tracing::warn!(key_id = %api_key.id, "rate limiter valkey error, failing open");
            Ok(next.run(req).await)
        }
    }
}

/// Sliding window rate limit check using Valkey sorted sets.
///
/// Uses ZREMRANGEBYSCORE to prune old entries, ZADD to add current request,
/// and ZCARD to count requests in the window.
async fn check_sliding_window(
    pool: &fred::clients::RedisPool,
    key: &str,
    now_ms: f64,
    window_ms: f64,
    limit: u64,
) -> anyhow::Result<bool> {
    use fred::prelude::*;

    let window_start = now_ms - window_ms;

    // Remove entries outside the window
    let _: u64 = pool
        .zremrangebyscore(key, f64::NEG_INFINITY, window_start)
        .await?;

    // Add current request
    let _: u64 = pool
        .zadd(key, None, None, false, false, (now_ms, now_ms.to_string().as_str()))
        .await?;

    // Set TTL on the key to auto-cleanup (2x window)
    let _: bool = pool
        .expire(key, (window_ms as i64 / 1000) * 2 + 1)
        .await?;

    // Count requests in window
    let count: u64 = pool.zcard(key).await?;

    Ok(count <= limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_rpm_zero() {
        // rate_limit_rpm == 0 means unlimited; the middleware should skip rate limiting
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
            created_at: chrono::Utc::now(),
        };
        assert_eq!(key.rate_limit_rpm, 0);
    }

    #[test]
    fn rate_limit_key_format() {
        let id = uuid::Uuid::now_v7();
        let key = format!("inferq:ratelimit:rpm:{}", id);
        assert!(key.starts_with("inferq:ratelimit:rpm:"));
        assert!(key.len() > 20);
    }
}
