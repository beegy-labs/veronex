/// Valkey heartbeat for provider liveness.
///
/// After each successful Ollama scrape the agent calls `set_online()` so
/// veronex can detect liveness from Valkey instead of probing every provider
/// with HTTP.  A missing key (TTL expired or never set) means offline.
///
/// Key layout mirrors veronex `valkey_keys::provider_heartbeat`:
///   `veronex:provider:hb:{provider_id}`  EX {ttl_secs}
///
/// TTL should be ≥ 3× scrape interval so two missed cycles don't flip
/// the provider offline.  Default: 3× (180s for 60s scrape interval).
use fred::clients::Pool;
use fred::prelude::*;

const HB_KEY_PREFIX: &str = "veronex:provider:hb:";
const AGENT_INSTANCES_SET: &str = "veronex:agent:instances";
const AGENT_HB_PREFIX: &str = "veronex:agent:hb:";
const VALKEY_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
        tracing::warn!(provider_id = %provider_id, error = %e, "heartbeat set failed");
    }
}

/// Register this agent pod in the global agent instance set and refresh heartbeat.
/// Called every scrape cycle. Other pods read SCARD to get dynamic replica count.
pub async fn register_agent(pool: &Pool, hostname: &str, ttl_secs: i64) {
    let _: Result<(), _> = pool.sadd(AGENT_INSTANCES_SET, hostname).await;
    let hb_key = format!("{AGENT_HB_PREFIX}{hostname}");
    let _: Result<(), _> = pool
        .set(&hb_key, "1", Some(Expiration::EX(ttl_secs)), None, false)
        .await;
}

/// Deregister this agent pod on graceful shutdown.
pub async fn deregister_agent(pool: &Pool, hostname: &str) {
    let _: Result<(), _> = pool.srem(AGENT_INSTANCES_SET, hostname).await;
    let hb_key = format!("{AGENT_HB_PREFIX}{hostname}");
    let _: Result<(), _> = pool.del(&hb_key).await;
    tracing::info!(hostname, "agent deregistered from Valkey");
}

/// Get dynamic replica count from the agent instance set.
/// Validates each member against its heartbeat key — removes stale members
/// whose HB key has expired and returns only the live count.
/// Falls back to `fallback` when Valkey is unavailable.
pub async fn dynamic_replicas(pool: &Pool, fallback: u32) -> u32 {
    let members: Result<Vec<String>, _> = pool.smembers(AGENT_INSTANCES_SET).await;
    let Ok(members) = members else {
        return fallback.max(1);
    };
    let mut live = 0u32;
    for member in &members {
        let hb_key = format!("{AGENT_HB_PREFIX}{member}");
        let exists: Result<bool, _> = pool.exists(&hb_key).await;
        if exists.unwrap_or(false) {
            live += 1;
        } else {
            // HB key expired — remove stale member from the set.
            let _: Result<(), _> = pool.srem(AGENT_INSTANCES_SET, member.as_str()).await;
        }
    }
    live.max(1)
}

/// Connect to Valkey and return a connection pool.
/// Returns `None` (and logs a warning) when connection fails.
///
/// `set_online` and `connect` are not unit-tested: they require a live Valkey
/// connection (external dependency → integration layer per testing-strategy.md).
pub async fn connect(url: &str) -> Option<Pool> {
    let config = match fred::types::config::Config::from_url(url) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "heartbeat: invalid VALKEY_URL");
            return None;
        }
    };
    let pool = match Builder::from_config(config)
        .with_connection_config(|c| {
            c.connection_timeout = VALKEY_CONNECT_TIMEOUT;
        })
        .build_pool(4)
    {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "heartbeat: failed to build Valkey pool");
            return None;
        }
    };
    // connect() spawns background tasks and returns a JoinHandle — drop it.
    let _ = pool.connect();
    if let Err(e) = pool.wait_for_connect().await {
        tracing::warn!(error = %e, "heartbeat: Valkey wait_for_connect failed");
        return None;
    }
    tracing::info!(url = %url, "heartbeat: connected to Valkey");
    Some(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// key() must produce the canonical format consumed by veronex MGET.
    /// veronex::valkey_keys::provider_heartbeat() generates the same prefix —
    /// this test guards against drift between the two crates.
    #[test]
    fn key_format_matches_veronex_convention() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(key(id), "veronex:provider:hb:550e8400-e29b-41d4-a716-446655440000");
    }
}
