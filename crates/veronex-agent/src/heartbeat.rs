/// Valkey heartbeat for provider liveness.
///
/// After each successful Ollama scrape the agent calls `set_online()` so
/// veronex can detect liveness from Valkey instead of probing every provider
/// with HTTP.  A missing key (TTL expired or never set) means offline.
///
/// Key layout mirrors veronex `valkey_keys::provider_heartbeat`:
///   `veronex:provider:hb:{provider_id}`  EX {ttl_secs}
///
/// TTL should be ≥ 2× scrape interval so a single missed cycle doesn't flip
/// the provider offline.  Default: 3× (180s for 60s scrape interval).
use fred::clients::Pool;
use fred::prelude::*;

const HB_KEY_PREFIX: &str = "veronex:provider:hb:";

/// Build the heartbeat key for a provider UUID string.
fn key(provider_id: &str) -> String {
    format!("{HB_KEY_PREFIX}{provider_id}")
}

/// Mark a provider as online.  Called after a successful `/api/ps` scrape.
/// Sets the heartbeat key with the given TTL (seconds).
pub async fn set_online(pool: &Pool, provider_id: &str, ttl_secs: i64) {
    let k = key(provider_id);
    let result: Result<(), _> = pool
        .set(
            &k,
            "1",
            Some(Expiration::EX(ttl_secs)),
            None,
            false,
        )
        .await;
    if let Err(e) = result {
        tracing::warn!(provider_id, "heartbeat set failed: {e}");
    }
}

/// Connect to Valkey and return a connection pool.
/// Returns `None` (and logs a warning) when connection fails.
pub async fn connect(url: &str) -> Option<Pool> {
    let config = match fred::types::config::Config::from_url(url) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("heartbeat: invalid VALKEY_URL: {e}");
            return None;
        }
    };
    let pool = match Builder::from_config(config)
        .with_connection_config(|c| {
            c.connection_timeout = std::time::Duration::from_secs(5);
        })
        .build_pool(4)
    {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("heartbeat: failed to build Valkey pool: {e}");
            return None;
        }
    };
    // connect() spawns background tasks and returns a JoinHandle — drop it.
    let _ = pool.connect();
    if let Err(e) = pool.wait_for_connect().await {
        tracing::warn!("heartbeat: Valkey wait_for_connect failed: {e}");
        return None;
    }
    tracing::info!(url, "heartbeat: connected to Valkey");
    Some(pool)
}
