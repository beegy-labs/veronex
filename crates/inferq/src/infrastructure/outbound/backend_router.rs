use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;

use crate::application::ports::outbound::inference_backend::InferenceBackendPort;
use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::{InferenceJob, InferenceResult, LlmBackend};
use crate::domain::enums::BackendType;
use crate::domain::value_objects::StreamToken;
use crate::infrastructure::outbound::gemini::GeminiAdapter;
use crate::infrastructure::outbound::ollama::OllamaAdapter;

// ── Static backend router (kept for tests) ─────────────────────────────────────

/// Routes inference calls to the appropriate backend adapter based on
/// `InferenceJob::backend`. Built at startup from a static set of adapters.
pub struct BackendRouter {
    backends: HashMap<BackendType, Arc<dyn InferenceBackendPort>>,
}

impl BackendRouter {
    pub fn builder() -> BackendRouterBuilder {
        BackendRouterBuilder::default()
    }

    fn get(&self, backend_type: &BackendType) -> Result<&Arc<dyn InferenceBackendPort>> {
        self.backends
            .get(backend_type)
            .ok_or_else(|| anyhow::anyhow!("no adapter registered for backend {:?}", backend_type))
    }
}

#[async_trait]
impl InferenceBackendPort for BackendRouter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        self.get(&job.backend)?.infer(job).await
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        match self.get(&job.backend) {
            Ok(backend) => backend.stream_tokens(job),
            Err(e) => Box::pin(async_stream::stream! {
                yield Err(e);
            }),
        }
    }
}

// ── Builder ────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct BackendRouterBuilder {
    backends: HashMap<BackendType, Arc<dyn InferenceBackendPort>>,
}

impl BackendRouterBuilder {
    pub fn register(
        mut self,
        backend_type: BackendType,
        adapter: Arc<dyn InferenceBackendPort>,
    ) -> Self {
        self.backends.insert(backend_type, adapter);
        self
    }

    pub fn build(self) -> BackendRouter {
        BackendRouter {
            backends: self.backends,
        }
    }
}

// ── Dynamic backend router ─────────────────────────────────────────────────────

/// Routes inference calls to backends registered in the database.
///
/// For Ollama: picks the server with the most available VRAM (via `/api/ps`).
/// For Gemini: picks the first active key (round-robin in future).
///
/// If no backend of the requested type is registered, the stream yields an error.
pub struct DynamicBackendRouter {
    registry: Arc<dyn LlmBackendRegistry>,
}

impl DynamicBackendRouter {
    pub fn new(registry: Arc<dyn LlmBackendRegistry>) -> Self {
        Self { registry }
    }

    /// Select the best available backend for the given type.
    /// Returns the `LlmBackend` record so callers can build a specific adapter.
    pub async fn pick_backend(&self, bt: &BackendType) -> Result<LlmBackend> {
        pick_best_backend(&*self.registry, bt).await
    }
}

#[async_trait]
impl InferenceBackendPort for DynamicBackendRouter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let cfg = pick_best_backend(&*self.registry, &job.backend).await?;
        make_adapter(&cfg).as_ref().infer(job).await
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let registry = self.registry.clone();
        let job = job.clone();

        Box::pin(async_stream::stream! {
            let cfg = match pick_best_backend(&*registry, &job.backend).await {
                Ok(c) => c,
                Err(e) => { yield Err(e); return; }
            };

            let adapter = make_adapter(&cfg);
            let mut s = adapter.stream_tokens(&job);
            while let Some(item) = s.next().await {
                yield item;
            }
        })
    }
}

// ── Backend selection helpers ──────────────────────────────────────────────────

/// Pick the best backend from the registry for the given type.
///
/// Ollama: selects the server with the most available VRAM.
///         If `total_vram_mb = 0` (unknown), the backend is always considered available.
/// Gemini: selects the first active key (simple round-robin).
pub async fn pick_best_backend(
    registry: &dyn LlmBackendRegistry,
    bt: &BackendType,
) -> Result<LlmBackend> {
    let all = registry.list_all().await?;
    let candidates: Vec<LlmBackend> = all
        .into_iter()
        .filter(|b| b.is_active && &b.backend_type == bt)
        .collect();

    if candidates.is_empty() {
        return Err(anyhow::anyhow!(
            "no registered backend for {:?} — register one via POST /v1/backends",
            bt
        ));
    }

    match bt {
        BackendType::Gemini => candidates
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no Gemini backend")),

        BackendType::Ollama => {
            // For each Ollama candidate, check available VRAM and pick the most free.
            let mut best: Option<(LlmBackend, i64)> = None;
            for b in candidates {
                let avail = get_ollama_available_vram_mb(&b).await;
                match &best {
                    None => best = Some((b, avail)),
                    Some((_, v)) if avail > *v => best = Some((b, avail)),
                    _ => {}
                }
            }
            best.map(|(b, _)| b)
                .ok_or_else(|| anyhow::anyhow!("no Ollama backend with available VRAM"))
        }
    }
}

/// Query Ollama's `/api/ps` endpoint and return available VRAM in MiB.
///
/// Returns `i64::MAX` if `total_vram_mb == 0` (VRAM size unknown → treat as unlimited).
/// Returns `0` on network/parse errors (treats backend as full).
pub async fn get_ollama_available_vram_mb(backend: &LlmBackend) -> i64 {
    if backend.total_vram_mb == 0 {
        // VRAM unknown → always consider this backend available.
        return i64::MAX;
    }

    let client = reqwest::Client::new();
    let url = format!("{}/api/ps", backend.url.trim_end_matches('/'));

    let Ok(resp) = client
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return 0;
    };

    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return 0;
    };

    // `size_vram` is in bytes; sum all loaded models.
    let used_bytes: i64 = json["models"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["size_vram"].as_i64())
        .sum();

    let used_mb = used_bytes / (1024 * 1024);
    backend.total_vram_mb - used_mb
}

/// Build a concrete inference adapter from a backend DB record.
pub fn make_adapter(cfg: &LlmBackend) -> Arc<dyn InferenceBackendPort> {
    match cfg.backend_type {
        BackendType::Ollama => Arc::new(OllamaAdapter::new(&cfg.url)),
        BackendType::Gemini => {
            let key = cfg.api_key_encrypted.as_deref().unwrap_or("");
            Arc::new(GeminiAdapter::new(key))
        }
    }
}
