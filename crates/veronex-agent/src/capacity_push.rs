/// Compute and push provider capacity state to Valkey.
///
/// After each successful Ollama scrape the agent:
///   1. Calls /api/show per loaded model to get architecture profile
///   2. Computes capacity state (arch params for KV estimation)
///   3. Pushes `veronex:provider:{id}:capacity_state` EX 180s
///
/// The veronex sync_loop reads this from Valkey instead of making O(N_models)
/// HTTP calls per provider, reducing sync latency at scale.
use std::time::Duration;

use fred::clients::Pool;
use fred::prelude::*;
use serde::{Deserialize, Serialize};

const SHOW_TIMEOUT: Duration = Duration::from_secs(10);
const CAPACITY_STATE_TTL: i64 = 180;
const CAPACITY_KEY_PREFIX: &str = "veronex:provider:";

fn capacity_key(provider_id: &str) -> String {
    format!("{CAPACITY_KEY_PREFIX}{provider_id}:capacity_state")
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LoadedModelCapacity {
    pub name: String,
    pub size_vram: u64,
    pub num_layers: u32,
    pub num_kv_heads: u32,
    pub head_dim: u32,
    pub max_ctx: u32,
    pub configured_ctx: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProviderCapacityState {
    pub loaded_models: Vec<LoadedModelCapacity>,
    pub total_vram_mb: u64,
    pub ts_ms: u64,
}

#[derive(Deserialize, Default)]
struct ShowResponse {
    model_info: Option<serde_json::Map<String, serde_json::Value>>,
    parameters: Option<String>,
}

async fn fetch_arch(
    client: &reqwest::Client,
    base_url: &str,
    model_name: &str,
) -> (u32, u32, u32, u32, u32) {
    // Returns (num_layers, num_kv_heads, head_dim, max_ctx, configured_ctx)
    // Returns all-zeros on failure (caller uses fallback).
    #[derive(Serialize)]
    struct ShowReq<'a> { name: &'a str }

    let resp: ShowResponse = match client
        .post(format!("{}/api/show", base_url.trim_end_matches('/')))
        .json(&ShowReq { name: model_name })
        .timeout(SHOW_TIMEOUT)
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!(model = %model_name, error = %e, "show parse failed");
                return (0, 0, 0, 0, 0);
            }
        },
        Err(e) => {
            tracing::debug!(model = %model_name, error = %e, "show request failed");
            return (0, 0, 0, 0, 0);
        }
    };

    let info = resp.model_info.unwrap_or_default();
    let find = |suffix: &str| -> u32 {
        info.iter()
            .find(|(k, _)| k.ends_with(suffix))
            .and_then(|(_, v)| v.as_u64())
            .unwrap_or(0) as u32
    };

    let block_count = find("block_count");
    let attn_interval = find("full_attention_interval");
    let num_layers = if attn_interval > 1 {
        block_count.div_ceil(attn_interval)
    } else {
        block_count
    };

    let kv_heads = find("attention.head_count_kv");
    let num_kv_heads = if kv_heads > 0 { kv_heads } else { find("attention.head_count") };

    let head_dim = find("attention.key_length").max(128);
    let max_ctx = find("context_length");

    let configured_ctx = resp
        .parameters
        .as_deref()
        .and_then(|p| {
            p.lines()
                .find(|l| l.starts_with("num_ctx"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0u32);

    (num_layers, num_kv_heads, head_dim, max_ctx, configured_ctx)
}

/// Compute and push capacity state for a provider.
/// `loaded_models`: (name, size_vram_bytes) from /api/ps scrape.
/// `total_vram_mb`: from the discovery target labels (provider DB field).
pub async fn push(
    client: &reqwest::Client,
    pool: &Pool,
    base_url: &str,
    provider_id: &str,
    total_vram_mb: u64,
    loaded_models: &[(String, u64)],
) {
    let mut models = Vec::with_capacity(loaded_models.len());

    for (name, size_vram) in loaded_models {
        let (num_layers, num_kv_heads, head_dim, max_ctx, configured_ctx) =
            fetch_arch(client, base_url, name).await;

        models.push(LoadedModelCapacity {
            name: name.clone(),
            size_vram: *size_vram,
            num_layers,
            num_kv_heads,
            head_dim,
            max_ctx,
            configured_ctx,
        });
    }

    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let state = ProviderCapacityState {
        loaded_models: models,
        total_vram_mb,
        ts_ms,
    };

    let json = match serde_json::to_string(&state) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(provider_id, error = %e, "capacity_push: serialize failed");
            return;
        }
    };

    let key = capacity_key(provider_id);
    let result: Result<(), _> = pool
        .set(&key, &json, Some(Expiration::EX(CAPACITY_STATE_TTL)), None, false)
        .await;

    if let Err(e) = result {
        tracing::warn!(provider_id, error = %e, "capacity_push: Valkey set failed");
    } else {
        tracing::debug!(provider_id, models = loaded_models.len(), "capacity state pushed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_key_format() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            capacity_key(id),
            "veronex:provider:550e8400-e29b-41d4-a716-446655440000:capacity_state"
        );
    }
}
