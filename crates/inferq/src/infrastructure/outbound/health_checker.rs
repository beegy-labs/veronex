use std::sync::Arc;
use std::time::Duration;

use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::LlmBackend;
use crate::domain::enums::{BackendType, LlmBackendStatus};

/// Check whether a single backend is reachable.
///
/// - Ollama: `GET {url}/api/version` → 200
/// - Gemini: lightweight models list with the stored API key → 200
pub async fn check_backend(client: &reqwest::Client, backend: &LlmBackend) -> LlmBackendStatus {
    match backend.backend_type {
        BackendType::Ollama => {
            let url = format!("{}/api/version", backend.url.trim_end_matches('/'));
            match client
                .get(&url)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
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
            match client
                .get(&url)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
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

/// Spawn a background task that checks all registered backends every `interval_secs` seconds
/// and updates their status to Online/Offline in the registry.
pub fn start_health_checker(
    registry: Arc<dyn LlmBackendRegistry>,
    interval_secs: u64,
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
            }
        }
    });
}
