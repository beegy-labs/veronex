use std::sync::Arc;
use std::time::Duration;

use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::LlmBackend;
use crate::domain::enums::{BackendType, LlmBackendStatus};
use crate::infrastructure::outbound::hw_metrics::{store_hw_metrics, HwMetrics};

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

/// Spawn a background task that checks all registered backends every `interval_secs`
/// seconds: updates online/offline status and (when `agent_url` is set) polls hardware
/// metrics and stores them in Valkey for the dispatcher.
pub fn start_health_checker(
    registry: Arc<dyn LlmBackendRegistry>,
    interval_secs: u64,
    valkey_pool: Option<fred::clients::RedisPool>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let interval = Duration::from_secs(interval_secs);

        tracing::info!("backend health checker started (interval={}s)", interval_secs);

        loop {
            tokio::time::sleep(interval).await;

            let backends = match registry.list_all().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("health checker: failed to list backends: {e}");
                    continue;
                }
            };

            let active: Vec<_> = backends.into_iter().filter(|b| b.is_active).collect();

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
                }
            }
        }
    });
}
