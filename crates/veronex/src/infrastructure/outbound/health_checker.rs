use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::ports::outbound::concurrency_port::VramPoolPort;
use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
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

/// Resolve node-exporter URL for a provider via its linked GpuServer.
///
/// Path: `provider.server_id` → `GpuServer.node_exporter_url`
async fn resolve_node_exporter_url(
    provider: &LlmProvider,
    gpu_server_registry: &dyn GpuServerRegistry,
) -> Option<String> {
    let server_id = provider.server_id?;
    let server = gpu_server_registry.get(server_id).await.ok()??;
    server.node_exporter_url.filter(|u| !u.is_empty())
}

/// Poll node-exporter via the provider's linked GpuServer, convert to
/// `HwMetrics`, and cache in Valkey.
async fn poll_node_exporter_metrics(
    provider: &LlmProvider,
    valkey_pool: &fred::clients::Pool,
    gpu_server_registry: &dyn GpuServerRegistry,
) {
    let Some(node_exporter_url) = resolve_node_exporter_url(provider, gpu_server_registry).await else {
        return;
    };

    let (node_metrics, _snapshot) = match fetch_node_metrics(&node_exporter_url, None).await {
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

    // DRM metrics come from amdgpu kernel driver — if DRM GPU exists, it's AMD.
    // NVIDIA GPUs use proprietary driver and don't expose DRM metrics via node-exporter.
    let gpu_vendor = if gpu.is_some() { "amd".to_string() } else { String::new() };

    let hw = HwMetrics {
        vram_used_mb: gpu.and_then(|g| g.vram_used_mb).unwrap_or(0) as u32,
        vram_total_mb: gpu.and_then(|g| g.vram_total_mb).unwrap_or(0) as u32,
        gpu_util_pct: gpu.and_then(|g| g.busy_pct).unwrap_or(0.0) as u8,
        power_w: gpu.and_then(|g| g.power_w).unwrap_or(0.0) as f32,
        temp_c: gpu.and_then(|g| g.temp_c).unwrap_or(0.0) as f32,
        temp_junction_c: gpu.and_then(|g| g.temp_junction_c).unwrap_or(0.0) as f32,
        temp_mem_c: gpu.and_then(|g| g.temp_mem_c).unwrap_or(0.0) as f32,
        mem_used_mb: (node_metrics.mem_total_mb.saturating_sub(node_metrics.mem_available_mb)) as u32,
        mem_total_mb: node_metrics.mem_total_mb as u32,
        mem_available_mb: node_metrics.mem_available_mb as u32,
        gpu_vendor,
    };

    tracing::debug!(
        provider_id = %provider.id,
        name = %provider.name,
        vram = "{}/{} MiB",
        hw.vram_used_mb, hw.vram_total_mb,
        temp = hw.max_temp_c(),
        "node-exporter metrics collected"
    );

    store_hw_metrics(valkey_pool, provider.id, &hw).await;
}

// ── Background task ────────────────────────────────────────────────────────────

/// Background loop that checks all registered providers every `interval_secs`
/// seconds: updates online/offline status and (when linked to a GpuServer with
/// node_exporter_url) polls hardware metrics and stores them in Valkey.
///
/// Also updates the `ThermalThrottleMap` on every cycle so the dispatcher can
/// respect soft/hard thermal limits without any additional network calls.
///
/// Exits cleanly when `shutdown` is cancelled.
/// Outcome of a single provider liveness check.
struct ProviderCheck {
    provider:   LlmProvider,
    new_status: LlmProviderStatus,
}

pub async fn run_health_checker_loop(
    registry:           Arc<dyn LlmProviderRegistry>,
    gpu_server_registry: Arc<dyn GpuServerRegistry>,
    interval_secs:      u64,
    valkey_pool:        Option<fred::clients::Pool>,
    thermal:            Arc<ThermalThrottleMap>,
    shutdown:           CancellationToken,
    client:             reqwest::Client,
    vram_pool:          Arc<dyn VramPoolPort>,
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

        // ── Determine liveness ────────────────────────────────────────────────
        //
        // When veronex-agent pushes heartbeats to Valkey (preferred at scale):
        //   MGET all heartbeat keys in one round-trip → no HTTP probing required.
        //
        // Fallback (when Valkey is absent or heartbeat key is missing):
        //   Direct HTTP probe via check_provider() — same behaviour as before.

        let checks: Vec<ProviderCheck> = if let Some(ref pool) = valkey_pool {
            use fred::prelude::*;

            // MGET all heartbeat keys in a single round-trip.
            // fred::mget returns Value::Array where each element is Null or a string.
            let keys: Vec<String> = active
                .iter()
                .map(|p| valkey_keys::provider_heartbeat(p.id))
                .collect();

            let mget_result: Result<Vec<Option<String>>, _> = pool.mget(keys).await;

            match mget_result {
                Ok(values) if values.len() == active.len() => {
                    // Derive status from presence of heartbeat key.
                    active
                        .into_iter()
                        .zip(values)
                        .map(|(provider, val): (LlmProvider, Option<String>)| {
                            let new_status = if val.is_some() {
                                LlmProviderStatus::Online
                            } else {
                                LlmProviderStatus::Offline
                            };
                            ProviderCheck { provider, new_status }
                        })
                        .collect()
                }
                Ok(_) | Err(_) => {
                    // Valkey unavailable or result length mismatch — fall back to HTTP.
                    tracing::warn!("health_checker: Valkey MGET failed or length mismatch, falling back to HTTP probes");
                    let mut checks = Vec::with_capacity(active.len());
                    for provider in active {
                        let new_status = check_provider(&client, &provider).await;
                        checks.push(ProviderCheck { provider, new_status });
                    }
                    checks
                }
            }
        } else {
            // No Valkey — probe each provider directly (concurrent, semaphore-limited).
            use tokio::task::JoinSet;
            use std::sync::Arc as StdArc;

            const MAX_CONCURRENT_PROBES: usize = 64;
            let sem = StdArc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PROBES));
            let mut set: JoinSet<ProviderCheck> = JoinSet::new();

            for provider in active {
                let client = client.clone();
                let sem = sem.clone();
                set.spawn(async move {
                    let _permit = sem.acquire_owned().await.expect("semaphore closed");
                    let new_status = check_provider(&client, &provider).await;
                    ProviderCheck { provider, new_status }
                });
            }
            let mut checks = Vec::new();
            while let Some(Ok(c)) = set.join_next().await {
                checks.push(c);
            }
            checks
        };

        // ── Apply status changes + HW metrics (sequential — avoids cache storm) ──

        let mut set: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

        for ProviderCheck { provider, new_status } in checks {
            let client            = client.clone();
            let registry          = registry.clone();
            let gpu_server_registry = gpu_server_registry.clone();
            let valkey_pool       = valkey_pool.clone();
            let thermal           = thermal.clone();
            let vram_pool         = vram_pool.clone();

            set.spawn(async move {
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
                    // Update O(1) online counter: avoid SELECT COUNT(*) in dashboard.
                    if let Some(ref pool) = valkey_pool {
                        use fred::prelude::*;
                        let delta: i64 = match (&provider.status, &new_status) {
                            (LlmProviderStatus::Online, _) => -1, // was online, now not
                            (_, LlmProviderStatus::Online) =>  1, // now online
                            _ => 0,
                        };
                        if delta != 0 {
                            if let Err(e) = pool
                                .incr_by::<i64, _>(valkey_keys::PROVIDERS_ONLINE_COUNTER, delta)
                                .await
                            {
                                tracing::warn!(
                                    provider_id = %provider.id,
                                    delta,
                                    "failed to update providers online counter: {e}"
                                );
                            }
                        }
                    }
                }

                // 2. Hardware metrics (only when linked to a GpuServer)
                if let Some(ref pool) = valkey_pool {
                    poll_node_exporter_metrics(&provider, pool, gpu_server_registry.as_ref()).await;

                    // 3. Thermal throttle update from cached hw_metrics
                    if let Some(hw) = load_hw_metrics(pool, provider.id).await {
                        use crate::infrastructure::outbound::capacity::thermal::ThermalThresholds;
                        let profile = match hw.gpu_vendor.as_str() {
                            "nvidia" => ThermalThresholds::GPU,
                            _ => ThermalThresholds::CPU,
                        };
                        thermal.set_thresholds(provider.id, profile);

                        let active_count = vram_pool.provider_active_requests(provider.id);
                        let sum_mc       = vram_pool.sum_loaded_max_concurrent(provider.id);
                        let prev  = thermal.get(provider.id);
                        let level = thermal.update(provider.id, hw.max_temp_c(), active_count, sum_mc);

                        if level != prev {
                            match &level {
                                ThrottleLevel::Hard => {
                                    tracing::warn!(
                                        provider = %provider.name,
                                        temp    = hw.max_temp_c(),
                                        cooldown_secs = 300,
                                        "HARD THROTTLE: dispatch suspended, cooldown active"
                                    );
                                    use fred::prelude::*;
                                    if let Err(e) = pool
                                        .set::<(), _, _>(
                                            &valkey_keys::thermal_throttle(provider.id),
                                            "hard",
                                            Some(Expiration::EX(360)),
                                            None,
                                            false,
                                        )
                                        .await
                                    {
                                        tracing::warn!(provider = %provider.name, "failed to set thermal throttle key: {e}");
                                    }
                                }
                                ThrottleLevel::Cooldown => {
                                    tracing::warn!(
                                        provider = %provider.name,
                                        temp    = hw.max_temp_c(),
                                        cooldown_secs = 300,
                                        "COOLDOWN: waiting for hardware to cool"
                                    );
                                }
                                ThrottleLevel::RampUp => {
                                    tracing::info!(
                                        provider = %provider.name,
                                        temp    = hw.max_temp_c(),
                                        "RAMP-UP: gradually restoring concurrency"
                                    );
                                }
                                ThrottleLevel::Soft => {
                                    tracing::warn!(
                                        provider = %provider.name,
                                        temp    = hw.max_temp_c(),
                                        "SOFT THROTTLE: new requests blocked"
                                    );
                                }
                                ThrottleLevel::Normal => {
                                    tracing::info!(
                                        provider = %provider.name,
                                        temp    = hw.max_temp_c(),
                                        "throttle lifted — normal ops"
                                    );
                                    use fred::prelude::*;
                                    if let Err(e) = pool
                                        .del::<(), _>(&valkey_keys::thermal_throttle(provider.id))
                                        .await
                                    {
                                        tracing::warn!(provider = %provider.name, "failed to del thermal throttle key: {e}");
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }

        // Drain all provider tasks before sleeping until the next cycle.
        while set.join_next().await.is_some() {}
    }

    tracing::info!("provider health checker stopped");
}
