use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{LlmProviderStatus, ProviderType};
use crate::infrastructure::outbound::capacity::thermal::{ThermalThrottleMap, ThrottleLevel};
use crate::infrastructure::outbound::hw_metrics::{load_hw_metrics, store_hw_metrics, HwMetrics};
use crate::infrastructure::outbound::gemini::adapter::GEMINI_BASE_URL;
use crate::infrastructure::outbound::valkey_keys;

// ── Agent response DTOs ────────────────────────────────────────────────────────

/// JSON shape returned by `veronex-agent GET /api/metrics`.
#[derive(serde::Deserialize)]
struct AgentMetrics {
    gpu: Option<AgentGpu>,
    memory: Option<AgentMemory>,
    ollama: Option<AgentOllama>,
}

#[derive(serde::Deserialize)]
struct AgentGpu {
    #[serde(default)]
    vram_used_mb: u32,
    #[serde(default)]
    vram_total_mb: u32,
    #[serde(default)]
    gpu_util_pct: u8,
    #[serde(default)]
    power_w: f32,
    #[serde(default)]
    temp_c: f32,
    #[serde(default)]
    gpu_vendor: String,
}

#[derive(serde::Deserialize)]
struct AgentMemory {
    #[serde(default)]
    used_mb: u32,
    #[serde(default)]
    total_mb: u32,
}

#[derive(serde::Deserialize)]
struct AgentOllama {
    #[serde(default)]
    loaded_model_count: u8,
}

use crate::domain::constants::{
    OLLAMA_HEALTH_CHECK_TIMEOUT as OLLAMA_HEALTH_TIMEOUT,
    GEMINI_HEALTH_CHECK_TIMEOUT as GEMINI_HEALTH_TIMEOUT,
    AGENT_METRICS_TIMEOUT,
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

// ── Agent metrics polling ──────────────────────────────────────────────────────

/// Poll `{agent_url}/api/metrics`, parse the response, and cache it in Valkey.
///
/// Errors are logged as warnings and do NOT affect the health status of the provider.
async fn poll_agent_metrics(
    client: &reqwest::Client,
    provider: &LlmProvider,
    valkey_pool: &fred::clients::Pool,
) {
    let Some(ref agent_url) = provider.agent_url else {
        return;
    };

    let url = format!("{}/api/metrics", agent_url.trim_end_matches('/'));

    let resp = match client.get(&url).timeout(AGENT_METRICS_TIMEOUT).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                provider_id = %provider.id,
                name = %provider.name,
                "agent metrics poll failed: {e}"
            );
            return;
        }
    };

    let agent: AgentMetrics = match resp.json().await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                provider_id = %provider.id,
                "failed to parse agent metrics: {e}"
            );
            return;
        }
    };

    let gpu = agent.gpu.unwrap_or(AgentGpu {
        vram_used_mb: 0,
        vram_total_mb: 0,
        gpu_util_pct: 0,
        power_w: 0.0,
        temp_c: 0.0,
        gpu_vendor: String::new(),
    });
    let mem = agent.memory.unwrap_or(AgentMemory { used_mb: 0, total_mb: 0 });
    let ollama = agent.ollama.unwrap_or(AgentOllama { loaded_model_count: 0 });

    let hw = HwMetrics {
        vram_used_mb: gpu.vram_used_mb,
        vram_total_mb: gpu.vram_total_mb,
        gpu_util_pct: gpu.gpu_util_pct,
        power_w: gpu.power_w,
        temp_c: gpu.temp_c,
        mem_used_mb: mem.used_mb,
        mem_total_mb: mem.total_mb,
        loaded_model_count: ollama.loaded_model_count,
        gpu_vendor: gpu.gpu_vendor,
    };

    tracing::debug!(
        provider_id = %provider.id,
        name = %provider.name,
        vram = "{}/{} MiB",
        hw.vram_used_mb, hw.vram_total_mb,
        temp = hw.temp_c,
        "agent metrics collected"
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

            // 2. Hardware metrics (only when agent_url is configured)
            if let Some(ref pool) = valkey_pool {
                poll_agent_metrics(&client, &provider, pool).await;

                // 3. Thermal throttle update from cached hw_metrics
                if let Some(hw) = load_hw_metrics(pool, provider.id).await {
                    // Set per-provider thermal profile from agent-reported gpu_vendor.
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
