use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;

use crate::application::ports::outbound::backend_model_selection::BackendModelSelectionRepository;
use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::inference_backend::InferenceBackendPort;
use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::domain::entities::{InferenceJob, InferenceResult, LlmBackend};
use crate::domain::enums::BackendType;
use crate::domain::value_objects::StreamToken;
use crate::infrastructure::outbound::gemini::GeminiAdapter;
use crate::infrastructure::outbound::hw_metrics::load_hw_metrics;
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
    model_selection_repo: Option<Arc<dyn BackendModelSelectionRepository>>,
    ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
}

impl DynamicBackendRouter {
    pub fn new(registry: Arc<dyn LlmBackendRegistry>) -> Self {
        Self { registry, model_selection_repo: None, ollama_model_repo: None }
    }

    pub fn with_model_selection(
        mut self,
        repo: Arc<dyn BackendModelSelectionRepository>,
    ) -> Self {
        self.model_selection_repo = Some(repo);
        self
    }

    pub fn with_ollama_model_repo(
        mut self,
        repo: Arc<dyn OllamaModelRepository>,
    ) -> Self {
        self.ollama_model_repo = Some(repo);
        self
    }

    /// Select the best available backend for the given type.
    /// Returns the `LlmBackend` record so callers can build a specific adapter.
    pub async fn pick_backend(&self, bt: &BackendType) -> Result<LlmBackend> {
        pick_best_backend(&*self.registry, None, self.model_selection_repo.as_deref(), self.ollama_model_repo.as_deref(), bt, "", None, None).await
    }
}

#[async_trait]
impl InferenceBackendPort for DynamicBackendRouter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let cfg = pick_best_backend(&*self.registry, None, self.model_selection_repo.as_deref(), self.ollama_model_repo.as_deref(), &job.backend, job.model_name.as_str(), None, None).await?;
        make_adapter(&cfg).as_ref().infer(job).await
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let registry = self.registry.clone();
        let model_selection_repo = self.model_selection_repo.clone();
        let ollama_model_repo = self.ollama_model_repo.clone();
        let job = job.clone();

        Box::pin(async_stream::stream! {
            let cfg = match pick_best_backend(&*registry, None, model_selection_repo.as_deref(), ollama_model_repo.as_deref(), &job.backend, job.model_name.as_str(), None, None).await {
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

// ── Gemini rate-limit helpers ──────────────────────────────────────────────────

/// Valkey key for per-(backend, model) RPM counter.
/// Bucketed by minute — TTL=120s so it always expires naturally.
fn gemini_rpm_key(backend_id: uuid::Uuid, model: &str) -> String {
    let minute = chrono::Utc::now().timestamp() / 60;
    format!("veronex:gemini:rpm:{}:{}:{}", backend_id, model, minute)
}

/// Valkey key for per-(backend, model) RPD counter.
/// Bucketed by UTC date — TTL=90000s (~25h).
fn gemini_rpd_key(backend_id: uuid::Uuid, model: &str) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    format!("veronex:gemini:rpd:{}:{}:{}", backend_id, model, date)
}

/// Returns `(rpm_exhausted, rpd_exhausted)` for a given free-tier backend + model.
/// Both false when Valkey is unavailable (fail-open).
async fn gemini_limit_status(
    backend_id: uuid::Uuid,
    model: &str,
    rpm_limit: i32,
    rpd_limit: i32,
    valkey: &fred::clients::Pool,
) -> (bool, bool) {
    use fred::prelude::*;

    let rpm_exhausted = if rpm_limit > 0 {
        let count: i64 = valkey
            .get::<Option<i64>, _>(gemini_rpm_key(backend_id, model))
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        count >= rpm_limit as i64
    } else {
        false
    };

    let rpd_exhausted = if rpd_limit > 0 {
        let count: i64 = valkey
            .get::<Option<i64>, _>(gemini_rpd_key(backend_id, model))
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        count >= rpd_limit as i64
    } else {
        false
    };

    (rpm_exhausted, rpd_exhausted)
}

/// Increment per-(backend, model) RPM and RPD counters after a successful inference.
pub async fn increment_gemini_counters(
    pool: &fred::clients::Pool,
    backend_id: uuid::Uuid,
    model: &str,
) -> anyhow::Result<()> {
    use fred::prelude::*;

    let rpm_key = gemini_rpm_key(backend_id, model);
    let rpd_key = gemini_rpd_key(backend_id, model);

    let _: i64 = pool.incr_by(&rpm_key, 1).await?;
    let _: bool = pool.expire(&rpm_key, 120, None).await?;

    let _: i64 = pool.incr_by(&rpd_key, 1).await?;
    let _: bool = pool.expire(&rpd_key, 90_000, None).await?;

    Ok(())
}

/// Pick the best backend from the registry for the given type and model.
///
/// `tier_filter` restricts which Gemini backends are considered:
///   - `Some("free")` — only `is_free_tier=true` backends; no paid fallback.
///   - `None` (default) — auto: free-tier first, paid fallback when exhausted.
///
/// Gemini dispatch rules:
///   - Free-tier backends checked first (registration order).
///   - RPD exhausted → skip this backend for today.
///   - RPM exhausted but RPD ok → ALL free-tier backends are RPM-limited:
///       sleep until next minute boundary, then retry (up to MAX_RPM_RETRIES times).
///   - All free-tier RPD-exhausted → fall back to paid (unless tier_filter="free").
///   - Paid backends: if model_selection_repo has rows for the backend and the
///     requested model is NOT enabled, that paid backend is skipped.
///
/// Ollama: picks the server with the most available VRAM.
pub async fn pick_best_backend(
    registry: &dyn LlmBackendRegistry,
    policy_repo: Option<&dyn GeminiPolicyRepository>,
    model_selection_repo: Option<&dyn BackendModelSelectionRepository>,
    ollama_model_repo: Option<&dyn OllamaModelRepository>,
    bt: &BackendType,
    model_name: &str,
    valkey: Option<&fred::clients::Pool>,
    tier_filter: Option<&str>,
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
        BackendType::Gemini => {
            pick_gemini_backend(candidates, policy_repo, model_selection_repo, model_name, valkey, tier_filter).await
        }

        BackendType::Ollama => {
            // Filter to backends that have the requested model synced (if DB is populated).
            let filtered_candidates = if let Some(repo) = ollama_model_repo {
                if !model_name.is_empty() {
                    match repo.backends_for_model(model_name).await {
                        Ok(ids) if !ids.is_empty() => {
                            let id_set: std::collections::HashSet<uuid::Uuid> =
                                ids.into_iter().collect();
                            let filtered: Vec<_> = candidates
                                .iter()
                                .filter(|b| id_set.contains(&b.id))
                                .cloned()
                                .collect();
                            if filtered.is_empty() {
                                // Model not found in DB — fall back to all candidates.
                                candidates
                            } else {
                                filtered
                            }
                        }
                        // DB empty or error → no filter, use all candidates.
                        _ => candidates,
                    }
                } else {
                    candidates
                }
            } else {
                candidates
            };

            // Filter by model selection: if a backend has selection rows for this model
            // and the model is disabled, skip that backend.
            let selection_filtered = if let Some(repo) = model_selection_repo {
                if !model_name.is_empty() {
                    let mut result = Vec::new();
                    for b in filtered_candidates {
                        match repo.list_enabled(b.id).await {
                            Ok(enabled) if !enabled.is_empty() => {
                                if enabled.iter().any(|m| m == model_name) {
                                    result.push(b);
                                } else {
                                    tracing::debug!(
                                        backend_id = %b.id,
                                        name = %b.name,
                                        model_name = %model_name,
                                        "model disabled on ollama backend, skipping"
                                    );
                                }
                            }
                            // No rows or error → no restriction, include this backend.
                            _ => result.push(b),
                        }
                    }
                    result
                } else {
                    filtered_candidates
                }
            } else {
                filtered_candidates
            };

            let mut best: Option<(LlmBackend, i64)> = None;
            for b in selection_filtered {
                let avail = get_ollama_available_vram_mb(&b, valkey).await;
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

/// Maximum times we will wait for an RPM window to reset before giving up.
const MAX_RPM_RETRIES: u32 = 3;

async fn pick_gemini_backend(
    candidates: Vec<LlmBackend>,
    policy_repo: Option<&dyn GeminiPolicyRepository>,
    model_selection_repo: Option<&dyn BackendModelSelectionRepository>,
    model_name: &str,
    valkey: Option<&fred::clients::Pool>,
    tier_filter: Option<&str>,
) -> Result<LlmBackend> {
    // Look up the shared rate-limit policy for this model.
    let policy = if let Some(repo) = policy_repo {
        repo.get_for_model(model_name).await.unwrap_or(None)
    } else {
        None
    };
    let rpm_limit = policy.as_ref().map(|p| p.rpm_limit).unwrap_or(0);
    let rpd_limit = policy.as_ref().map(|p| p.rpd_limit).unwrap_or(0);
    let available_on_free_tier = policy.as_ref().map(|p| p.available_on_free_tier).unwrap_or(true);

    let (free_backends_all, raw_paid_backends_all): (Vec<_>, Vec<_>) =
        candidates.into_iter().partition(|b| b.is_free_tier);

    // Apply tier filter: restrict which pools are considered.
    let (free_backends, raw_paid_backends) = match tier_filter {
        Some("free") => (free_backends_all, Vec::new()),  // free only, no paid fallback
        _ => (free_backends_all, raw_paid_backends_all),  // auto: free-first, paid-fallback
    };

    // Filter paid backends by model selection: if a backend has selection rows
    // and the requested model is not enabled, skip that backend.
    let mut paid_backends: Vec<LlmBackend> = Vec::new();
    for b in raw_paid_backends {
        if let Some(repo) = model_selection_repo {
            match repo.list_enabled(b.id).await {
                Ok(enabled) if !enabled.is_empty() => {
                    if enabled.iter().any(|m| m == model_name) {
                        paid_backends.push(b);
                    } else {
                        tracing::debug!(
                            backend_id = %b.id,
                            name = %b.name,
                            model_name = %model_name,
                            "model not in paid backend's enabled list, skipping"
                        );
                    }
                }
                // No rows or error → no restriction, include this backend.
                _ => paid_backends.push(b),
            }
        } else {
            paid_backends.push(b);
        }
    }

    // If the model is not available on free tier, skip free-tier backends entirely.
    if !available_on_free_tier {
        if tier_filter == Some("free") {
            return Err(anyhow::anyhow!(
                "model '{}' is not available on free tier (policy restriction)",
                model_name
            ));
        }
        if let Some(paid) = paid_backends.first() {
            tracing::info!(
                model_name = %model_name,
                backend_id = %paid.id,
                name = %paid.name,
                "model not available on free tier, routing directly to paid backend"
            );
            return Ok(paid.clone());
        }
        return Err(anyhow::anyhow!(
            "model '{}' requires a paid Gemini backend but none is configured",
            model_name
        ));
    }

    for attempt in 0..=MAX_RPM_RETRIES {
        let Some(pool) = valkey else {
            // No Valkey → skip rate-limit checks entirely, use first free or paid.
            if let Some(b) = free_backends.first() {
                return Ok(b.clone());
            }
            if let Some(b) = paid_backends.first() {
                return Ok(b.clone());
            }
            return Err(anyhow::anyhow!("no active Gemini backend available"));
        };

        let mut all_rpd_exhausted = !free_backends.is_empty();
        let mut any_rpm_available = false;

        for b in &free_backends {
            let (rpm_ex, rpd_ex) =
                gemini_limit_status(b.id, model_name, rpm_limit, rpd_limit, pool).await;

            if rpd_ex {
                tracing::info!(backend_id = %b.id, name = %b.name,
                    "Gemini backend RPD exhausted for today, skipping");
                continue;
            }

            // This key still has daily quota.
            all_rpd_exhausted = false;

            if !rpm_ex {
                // Found a key with both RPM and RPD available.
                return Ok(b.clone());
            }

            // RPM-limited but RPD still ok → flag that we might retry after waiting.
            any_rpm_available = true;
        }

        // All free keys are RPD-exhausted → fall back to paid.
        if all_rpd_exhausted {
            if let Some(paid) = paid_backends.first() {
                tracing::info!(backend_id = %paid.id, name = %paid.name,
                    "all Gemini free-tier backends RPD-exhausted, using paid backend");
                return Ok(paid.clone());
            }
            return Err(anyhow::anyhow!(
                "all Gemini free-tier backends exhausted daily quota and no paid backend configured"
            ));
        }

        // Some free keys have RPD quota but all are currently RPM-limited.
        // Wait until the next minute boundary, then retry.
        if any_rpm_available && attempt < MAX_RPM_RETRIES {
            let now_secs = chrono::Utc::now().timestamp();
            let secs_to_next_minute = 60 - (now_secs % 60) + 1; // +1 buffer
            tracing::info!(
                wait_secs = secs_to_next_minute,
                attempt = attempt + 1,
                "all Gemini free-tier backends RPM-limited, waiting for next minute"
            );
            tokio::time::sleep(Duration::from_secs(secs_to_next_minute as u64)).await;
            continue;
        }

        break;
    }

    // Retries exhausted.
    if let Some(paid) = paid_backends.first() {
        tracing::warn!(backend_id = %paid.id, "Gemini RPM retries exhausted, falling back to paid");
        return Ok(paid.clone());
    }

    Err(anyhow::anyhow!(
        "all Gemini free-tier backends are RPM-limited and no paid backend available"
    ))
}

/// Return available VRAM in MiB for an Ollama backend.
///
/// Priority:
/// 1. Valkey hardware metrics cache (set by health_checker when agent_url is configured).
///    Also enforces a temperature guard: backends at or above 85 °C are treated as
///    unavailable (returns `i64::MIN`).
/// 2. Live Ollama `/api/ps` poll (fallback when no agent data is cached).
/// 3. `i64::MAX` when `total_vram_mb == 0` (VRAM unknown → treat as unlimited).
/// 4. `0` on any network / parse error (treats backend as full).
pub async fn get_ollama_available_vram_mb(
    backend: &LlmBackend,
    valkey: Option<&fred::clients::Pool>,
) -> i64 {
    // ── 1. Valkey cache (agent data) ─────────────────────────────────────────
    if let Some(pool) = valkey {
        if let Some(hw) = load_hw_metrics(pool, backend.id).await {
            if hw.is_overheating() {
                tracing::warn!(
                    backend_id = %backend.id,
                    name = %backend.name,
                    temp = hw.temp_c,
                    "backend overheating — skipping dispatch"
                );
                return i64::MIN;
            }
            if hw.vram_total_mb > 0 {
                return hw.vram_free_mb();
            }
        }
    }

    // ── 2. VRAM unknown → treat as unlimited ─────────────────────────────────
    if backend.total_vram_mb == 0 {
        return i64::MAX;
    }

    // ── 3. Live /api/ps fallback ─────────────────────────────────────────────
    let client = reqwest::Client::new();
    let url = format!("{}/api/ps", backend.url.trim_end_matches('/'));

    let Ok(resp) = client.get(&url).timeout(Duration::from_secs(3)).send().await else {
        return 0;
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return 0;
    };

    let used_bytes: i64 = json["models"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["size_vram"].as_i64())
        .sum();

    backend.total_vram_mb - used_bytes / (1024 * 1024)
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
