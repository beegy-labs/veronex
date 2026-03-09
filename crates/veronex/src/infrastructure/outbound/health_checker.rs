use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{LlmProviderStatus, ProviderType};
use crate::infrastructure::outbound::capacity::thermal::{ThermalThrottleMap, ThrottleLevel};
use crate::infrastructure::outbound::hw_metrics::{load_hw_metrics, store_hw_metrics, fetch_node_metrics, HwMetrics};
use crate::infrastructure::outbound::gemini::adapter::GEMINI_BASE_URL;
use crate::infrastructure::outbound::valkey_keys;

use crate::domain::constants::{
    OLLAMA_HEALTH_CHECK_TIMEOUT as OLLAMA_HEALTH_TIMEOUT,
    GEMINI_HEALTH_CHECK_TIMEOUT as GEMINI_HEALTH_TIMEOUT,
};

// ── Health check ───────────────────────────────────────────────────────────────

/// Check whether a single provider is reachable.
///
/// - Ollama: `GET {url}/api/version` → 200
/// - Gemini: lightweight models list with the stored API key → 200
pub async fn check_provider(client: &reqwest::Client, provider: &LlmProvider) -> LlmProviderStatus {
    match provider.provider_type {
        ProviderType::Ollama => {
            let url = format!("{}/api/version", provider.url.trim_end_matches('/'));
            match client.get(&url).timeout(OLLAMA_HEALTH_TIMEOUT).send().await {
                Ok(r) if r.status().is_success() => LlmProviderStatus::Online,
                Ok(r) => {
                    tracing::warn!(
                        provider_id = %provider.id,
                        status = %r.status(),
                        "Ollama health check returned non-2xx"
                    );
                    LlmProviderStatus::Offline
                }
                Err(e) => {
                    tracing::warn!(provider_id = %provider.id, "Ollama health check failed: {e}");
                    LlmProviderStatus::Offline
                }
            }
        }
        ProviderType::Gemini => {
            let Some(ref key) = provider.api_key_encrypted else {
                tracing::warn!(provider_id = %provider.id, "Gemini provider has no API key");
                return LlmProviderStatus::Offline;
            };
            let url = format!("{GEMINI_BASE_URL}/v1beta/models?pageSize=1");
            match client.get(&url)
                .header("x-goog-api-key", key.as_str())
                .timeout(GEMINI_HEALTH_TIMEOUT)
                .send().await {
                Ok(r) if r.status().is_success() => LlmProviderStatus::Online,
                Ok(r) => {
                    tracing::warn!(
                        provider_id = %provider.id,
                        status = %r.status(),
                        "Gemini health check returned non-2xx"
                    );
                    LlmProviderStatus::Offline
                }
                Err(e) => {
                    tracing::warn!(provider_id = %provider.id, "Gemini health check failed: {e}");
                    LlmProviderStatus::Offline
                }
            }
        }
    }
}

// ── Node-exporter metrics polling ─────────────────────────────────────────────

/// Poll node-exporter via the provider's `agent_url` (node-exporter URL),
/// convert to `HwMetrics`, and cache in Valkey.
///
async fn poll_node_exporter_metrics(
    provider: &LlmProvider,
    valkey_pool: &fred::clients::Pool,
) {
    let Some(ref node_exporter_url) = provider.agent_url else {
        return;
    };

    let (node_metrics, _snapshot) = match fetch_node_metrics(node_exporter_url, None).await {
        Ok(result) => result,
        Err(e) => {
            tracing::warn!(
                provider_id = %provider.id,
                name = %provider.name,
                "node-exporter metrics poll failed: {e}"
            );
            return;
        }
    };

    let gpu_idx = provider.gpu_index.unwrap_or(0) as usize;
    let gpu = node_metrics.gpus.get(gpu_idx);

    // Detect GPU vendor from hwmon chip presence (amdgpu detection in parser)
    let gpu_vendor = if gpu.is_some() { "amd".to_string() } else { String::new() };

    let hw = HwMetrics {
        vram_used_mb: gpu.and_then(|g| g.vram_used_mb).unwrap_or(0) as u32,
        vram_total_mb: gpu.and_then(|g| g.vram_total_mb).unwrap_or(0) as u32,
        gpu_util_pct: gpu.and_then(|g| g.busy_pct).unwrap_or(0.0) as u8,
        power_w: gpu.and_then(|g| g.power_w).unwrap_or(0.0) as f32,
        temp_c: gpu.and_then(|g| g.temp_c).unwrap_or(0.0) as f32,
        mem_used_mb: (node_metrics.mem_total_mb.saturating_sub(node_metrics.mem_available_mb)) as u32,
        mem_total_mb: node_metrics.mem_total_mb as u32,
        gpu_vendor,
    };

    tracing::debug!(
        provider_id = %provider.id,
        name = %provider.name,
        vram = "{}/{} MiB",
        hw.vram_used_mb, hw.vram_total_mb,
        temp = hw.temp_c,
        "node-exporter metrics collected"
    );

    store_hw_metrics(valkey_pool, provider.id, &hw).await;
}

// ── Background task ────────────────────────────────────────────────────────────

/// Background loop that checks all registered providers every `interval_secs`
/// seconds: updates online/offline status and (when `agent_url` is set) polls hardware
/// metrics and stores them in Valkey for the dispatcher.
///
/// Also updates the `ThermalThrottleMap` on every cycle so the dispatcher can
/// respect soft/hard thermal limits without any additional network calls.
///
/// Exits cleanly when `shutdown` is cancelled.
pub async fn run_health_checker_loop(
    registry:    Arc<dyn LlmProviderRegistry>,
    interval_secs: u64,
    valkey_pool: Option<fred::clients::Pool>,
    thermal:     Arc<ThermalThrottleMap>,
    shutdown:    CancellationToken,
    client:      reqwest::Client,
) {
    let interval = Duration::from_secs(interval_secs);

    tracing::info!("provider health checker started (interval={}s)", interval_secs);

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }

        let providers = match registry.list_all().await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("health checker: failed to list providers: {e}");
                continue;
            }
        };

        // Only auto-check Ollama providers. Gemini status is updated manually
        // via POST /v1/gemini/sync-status to avoid unnecessary API quota usage.
        let active: Vec<_> = providers
            .into_iter()
            .filter(|b| b.is_active && matches!(b.provider_type, ProviderType::Ollama))
            .collect();

        for provider in active {
            // 1. Connectivity health check
            let new_status = check_provider(&client, &provider).await;
            if new_status != provider.status {
                tracing::info!(
                    provider_id = %provider.id,
                    name = %provider.name,
                    old = ?provider.status,
                    new = ?new_status,
                    "provider status changed"
                );
                if let Err(e) = registry.update_status(provider.id, new_status).await {
                    tracing::warn!(provider_id = %provider.id, "failed to update status: {e}");
                }
            }

            // 2. Hardware metrics (only when node-exporter URL is configured)
            if let Some(ref pool) = valkey_pool {
                poll_node_exporter_metrics(&provider, pool).await;

                // 3. Thermal throttle update from cached hw_metrics
                if let Some(hw) = load_hw_metrics(pool, provider.id).await {
                    // Set per-provider thermal profile from gpu_vendor.
                    // NVIDIA discrete GPUs tolerate higher temps; AMD APU (Ryzen AI 395+)
                    // uses CPU-class thresholds (more conservative).
                    use crate::infrastructure::outbound::capacity::thermal::ThermalThresholds;
                    let profile = match hw.gpu_vendor.as_str() {
                        "nvidia" => ThermalThresholds::GPU,
                        _ => ThermalThresholds::CPU, // amd (APU/iGPU), unknown, empty
                    };
                    thermal.set_thresholds(provider.id, profile);

                    let prev  = thermal.get(provider.id);
                    let level = thermal.update(provider.id, hw.temp_c);

                    if level != prev {
                        match &level {
                            ThrottleLevel::Hard => {
                                tracing::warn!(
                                    provider = %provider.name,
                                    temp    = hw.temp_c,
                                    cooldown_secs = 60,
                                    "HARD THROTTLE: dispatch suspended, cooldown active"
                                );
                                use fred::prelude::*;
                                let _: () = pool
                                    .set(
                                        &valkey_keys::thermal_throttle(provider.id),
                                        "hard",
                                        Some(Expiration::EX(90)),
                                        None,
                                        false,
                                    )
                                    .await
                                    .unwrap_or(());
                            }
                            ThrottleLevel::Soft => {
                                tracing::warn!(
                                    provider = %provider.name,
                                    temp    = hw.temp_c,
                                    "SOFT THROTTLE: capped to 1 slot"
                                );
                            }
                            ThrottleLevel::Normal => {
                                tracing::info!(
                                    provider = %provider.name,
                                    temp    = hw.temp_c,
                                    "throttle lifted — normal ops"
                                );
                                use fred::prelude::*;
                                let _: () = pool
                                    .del::<(), _>(&valkey_keys::thermal_throttle(provider.id))
                                    .await
                                    .unwrap_or(());
                            }
                        }
                    }
                }
            }
        }
    }

    tracing::info!("provider health checker stopped");
}
