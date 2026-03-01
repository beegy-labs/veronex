use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::LlmBackend;
use crate::domain::enums::{BackendType, LlmBackendStatus};
use crate::infrastructure::outbound::capacity::thermal::{ThermalThrottleMap, ThrottleLevel};
use crate::infrastructure::outbound::hw_metrics::{load_hw_metrics, store_hw_metrics, HwMetrics};

// ── Agent response DTOs ────────────────────────────────────────────────────────

/// JSON shape returned by `inferq-agent GET /api/metrics`.
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

// ── Health check ───────────────────────────────────────────────────────────────

/// Check whether a single backend is reachable.
///
/// - Ollama: `GET {url}/api/version` → 200
/// - Gemini: lightweight models list with the stored API key → 200
pub async fn check_backend(client: &reqwest::Client, backend: &LlmBackend) -> LlmBackendStatus {
    match backend.backend_type {
        BackendType::Ollama => {
            let url = format!("{}/api/version", backend.url.trim_end_matches('/'));
            match client.get(&url).timeout(Duration::from_secs(5)).send().await {
                Ok(r) if r.status().is_success() => LlmBackendStatus::Online,
                Ok(r) => {
                    tracing::warn!(
                        backend_id = %backend.id,
                        status = %r.status(),
                        "Ollama health check returned non-2xx"
                    );
                    LlmBackendStatus::Offline
                }
                Err(e) => {
                    tracing::warn!(backend_id = %backend.id, "Ollama health check failed: {e}");
                    LlmBackendStatus::Offline
                }
            }
        }
        BackendType::Gemini => {
            let Some(ref key) = backend.api_key_encrypted else {
                tracing::warn!(backend_id = %backend.id, "Gemini backend has no API key");
                return LlmBackendStatus::Offline;
            };
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models?key={key}&pageSize=1"
            );
            match client.get(&url).timeout(Duration::from_secs(10)).send().await {
                Ok(r) if r.status().is_success() => LlmBackendStatus::Online,
                Ok(r) => {
                    tracing::warn!(
                        backend_id = %backend.id,
                        status = %r.status(),
                        "Gemini health check returned non-2xx"
                    );
                    LlmBackendStatus::Offline
                }
                Err(e) => {
                    tracing::warn!(backend_id = %backend.id, "Gemini health check failed: {e}");
                    LlmBackendStatus::Offline
                }
            }
        }
    }
}

// ── Agent metrics polling ──────────────────────────────────────────────────────

/// Poll `{agent_url}/api/metrics`, parse the response, and cache it in Valkey.
///
/// Errors are logged as warnings and do NOT affect the health status of the backend.
async fn poll_agent_metrics(
    client: &reqwest::Client,
    backend: &LlmBackend,
    valkey_pool: &fred::clients::RedisPool,
) {
    let Some(ref agent_url) = backend.agent_url else {
        return;
    };

    let url = format!("{}/api/metrics", agent_url.trim_end_matches('/'));

    let resp = match client.get(&url).timeout(Duration::from_secs(5)).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                backend_id = %backend.id,
                name = %backend.name,
                "agent metrics poll failed: {e}"
            );
            return;
        }
    };

    let agent: AgentMetrics = match resp.json().await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                backend_id = %backend.id,
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
    };

    tracing::debug!(
        backend_id = %backend.id,
        name = %backend.name,
        vram = "{}/{} MiB",
        hw.vram_used_mb, hw.vram_total_mb,
        temp = hw.temp_c,
        "agent metrics collected"
    );

    store_hw_metrics(valkey_pool, backend.id, &hw).await;
}

// ── Background task ────────────────────────────────────────────────────────────

/// Background loop that checks all registered backends every `interval_secs`
/// seconds: updates online/offline status and (when `agent_url` is set) polls hardware
/// metrics and stores them in Valkey for the dispatcher.
///
/// Also updates the `ThermalThrottleMap` on every cycle so the dispatcher can
/// respect soft/hard thermal limits without any additional network calls.
///
/// Exits cleanly when `shutdown` is cancelled.
pub async fn run_health_checker_loop(
    registry:    Arc<dyn LlmBackendRegistry>,
    interval_secs: u64,
    valkey_pool: Option<fred::clients::RedisPool>,
    thermal:     Arc<ThermalThrottleMap>,
    shutdown:    CancellationToken,
) {
    let client = reqwest::Client::new();
    let interval = Duration::from_secs(interval_secs);

    tracing::info!("backend health checker started (interval={}s)", interval_secs);

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(interval) => {}
        }

        let backends = match registry.list_all().await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("health checker: failed to list backends: {e}");
                continue;
            }
        };

        // Only auto-check Ollama backends. Gemini status is updated manually
        // via POST /v1/gemini/sync-status to avoid unnecessary API quota usage.
        let active: Vec<_> = backends
            .into_iter()
            .filter(|b| b.is_active && matches!(b.backend_type, BackendType::Ollama))
            .collect();

        for backend in active {
            // 1. Connectivity health check
            let new_status = check_backend(&client, &backend).await;
            if new_status != backend.status {
                tracing::info!(
                    backend_id = %backend.id,
                    name = %backend.name,
                    old = ?backend.status,
                    new = ?new_status,
                    "backend status changed"
                );
                if let Err(e) = registry.update_status(backend.id, new_status).await {
                    tracing::warn!(backend_id = %backend.id, "failed to update status: {e}");
                }
            }

            // 2. Hardware metrics (only when agent_url is configured)
            if let Some(ref pool) = valkey_pool {
                poll_agent_metrics(&client, &backend, pool).await;

                // 3. Thermal throttle update from cached hw_metrics
                if let Some(hw) = load_hw_metrics(pool, backend.id).await {
                    let prev  = thermal.get(backend.id);
                    let level = thermal.update(backend.id, hw.temp_c);

                    if level != prev {
                        match &level {
                            ThrottleLevel::Hard => {
                                tracing::warn!(
                                    backend = %backend.name,
                                    temp    = hw.temp_c,
                                    cooldown_secs = 60,
                                    "HARD THROTTLE: dispatch suspended, cooldown active"
                                );
                                use fred::prelude::*;
                                let _: () = pool
                                    .set(
                                        &format!("veronex:throttle:{}", backend.id),
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
                                    backend = %backend.name,
                                    temp    = hw.temp_c,
                                    "SOFT THROTTLE: capped to 1 slot"
                                );
                            }
                            ThrottleLevel::Normal => {
                                tracing::info!(
                                    backend = %backend.name,
                                    temp    = hw.temp_c,
                                    "throttle lifted — normal ops"
                                );
                                use fred::prelude::*;
                                let _: () = pool
                                    .del::<(), _>(&format!(
                                        "veronex:throttle:{}",
                                        backend.id
                                    ))
                                    .await
                                    .unwrap_or(());
                            }
                        }
                    }
                }
            }
        }
    }

    tracing::info!("backend health checker stopped");
}
