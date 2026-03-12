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
/// On success: `is_loaded=true`, `preload_fail_count=0`, `preload_failed_at=0`,
/// and sets initial `max_concurrent` using committed_parallel calculation.
/// On failure: `preload_fail_count += 1`; at 3 consecutive failures → 300s exclusion.
pub async fn preload_model(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    provider_id: Uuid,
    vram_pool: &Arc<dyn VramPoolPort>,
    num_parallel: u32,
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
            // committed_parallel: initial max_concurrent for newly loaded model
            let committed = vram_pool.sum_loaded_max_concurrent(provider_id);
            let initial = num_parallel.min(num_parallel.saturating_sub(committed)).max(1);
            vram_pool.set_max_concurrent(provider_id, model, initial);
            tracing::info!(
                %provider_id, %model, initial, committed,
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::infrastructure::outbound::capacity::vram_pool::VramPool;
    use crate::application::ports::outbound::concurrency_port::VramPoolPort;

    #[test]
    fn committed_parallel_reduces_initial_for_second_model() {
        // num_parallel=8, first model max_concurrent=4 already loaded
        // → new model initial = min(8, 8-4) = 4
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "model_a", 1000);
        pool.set_max_concurrent(pid, "model_a", 4);

        let committed = pool.sum_loaded_max_concurrent(pid);
        let num_parallel = 8u32;
        let initial = num_parallel.min(num_parallel.saturating_sub(committed)).max(1);
        assert_eq!(initial, 4);
    }

    #[test]
    fn committed_parallel_min_1_when_fully_committed() {
        // num_parallel=4, loaded models total max_concurrent=4
        // → new model initial = max(1, min(4, 4-4)) = max(1, 0) = 1
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "model_a", 1000);
        pool.set_max_concurrent(pid, "model_a", 4);

        let committed = pool.sum_loaded_max_concurrent(pid);
        let num_parallel = 4u32;
        let initial = num_parallel.min(num_parallel.saturating_sub(committed)).max(1);
        assert_eq!(initial, 1); // floor at 1, not 0
    }

    #[test]
    fn committed_parallel_first_model_gets_full_parallel() {
        // No other loaded models → initial = min(8, 8-0) = 8
        let pool = VramPool::new();
        let pid = Uuid::now_v7();

        let committed = pool.sum_loaded_max_concurrent(pid);
        let num_parallel = 8u32;
        let initial = num_parallel.min(num_parallel.saturating_sub(committed)).max(1);
        assert_eq!(initial, 8);
    }

    #[test]
    fn committed_parallel_overcrowded_saturates_to_1() {
        // loaded max_concurrent sum > num_parallel (overcrowded)
        // saturating_sub prevents underflow → initial = max(1, 0) = 1
        let pool = VramPool::new();
        let pid = Uuid::now_v7();
        pool.mark_model_loaded(pid, "a", 1000);
        pool.set_max_concurrent(pid, "a", 6);
        pool.mark_model_loaded(pid, "b", 1000);
        pool.set_max_concurrent(pid, "b", 6);

        let committed = pool.sum_loaded_max_concurrent(pid);
        let num_parallel = 8u32;
        let initial = num_parallel.min(num_parallel.saturating_sub(committed)).max(1);
        assert_eq!(initial, 1);
    }
}
