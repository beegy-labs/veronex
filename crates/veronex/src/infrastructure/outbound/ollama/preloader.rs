//! Preloader: trigger model load on Ollama before dispatching inference requests.
//!
//! Phase 6: sends a zero-token generate request (`num_predict: 0, keep_alive: -1`)
//! to load the model into VRAM. On success, marks `is_loaded=true` and resets
//! failure counters. On 3 consecutive failures, the model+provider pair is
//! excluded from routing for 300s.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::VramPoolPort;

/// Timeout for preload requests (model loading can take minutes for large models).
const PRELOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Trigger model preload on an Ollama provider.
///
/// Sets `is_preloading=true` before the request, restores to `false` on completion.
/// On success: `is_loaded=true`, `preload_fail_count=0`, `preload_failed_at=0`.
/// On failure: `preload_fail_count += 1`; at 3 consecutive failures → 300s exclusion.
pub async fn preload_model(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    provider_id: Uuid,
    vram_pool: &Arc<dyn VramPoolPort>,
) -> bool {
    vram_pool.set_preloading(provider_id, model, true);

    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let result = client
        .post(&url)
        .json(&serde_json::json!({
            "model": model,
            "prompt": "",
            "num_predict": 0,
            "keep_alive": -1
        }))
        .timeout(PRELOAD_TIMEOUT)
        .send()
        .await;

    vram_pool.set_preloading(provider_id, model, false);

    match result {
        Ok(resp) if resp.status().is_success() => {
            vram_pool.record_preload_success(provider_id, model);
            tracing::info!(
                %provider_id, %model,
                "preload successful — model loaded into VRAM"
            );
            true
        }
        Ok(resp) => {
            tracing::warn!(
                %provider_id, %model, status = %resp.status(),
                "preload failed — non-success status"
            );
            vram_pool.record_preload_failure(provider_id, model);
            false
        }
        Err(e) => {
            tracing::warn!(
                %provider_id, %model,
                "preload failed: {e}"
            );
            vram_pool.record_preload_failure(provider_id, model);
            false
        }
    }
}
