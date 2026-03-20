use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;

use crate::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::inference_provider::InferenceProviderPort;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::domain::entities::{InferenceJob, InferenceResult, LlmProvider};
use crate::domain::enums::ProviderType;
use crate::domain::value_objects::StreamToken;
use crate::infrastructure::outbound::gemini::GeminiAdapter;
use crate::infrastructure::outbound::hw_metrics::load_hw_metrics;
use crate::infrastructure::outbound::ollama::OllamaAdapter;
use crate::infrastructure::outbound::valkey_keys;

/// TTL (seconds) for the per-minute Gemini RPM counter key.
const GEMINI_RPM_TTL_SECS: i64 = 120;

/// TTL (seconds) for the per-day Gemini RPD counter key (~25 hours).
const GEMINI_RPD_TTL_SECS: i64 = 90_000;

// ── Static provider router (kept for tests) ────────────────────────────────────

/// Routes inference calls to the appropriate provider adapter based on
/// `InferenceJob::provider_type`. Built at startup from a static set of adapters.
pub struct ProviderRouter {
    providers: HashMap<ProviderType, Arc<dyn InferenceProviderPort>>,
}

impl ProviderRouter {
    pub fn builder() -> ProviderRouterBuilder {
        ProviderRouterBuilder::default()
    }

    fn get(&self, provider_type: &ProviderType) -> Result<&Arc<dyn InferenceProviderPort>> {
        self.providers
            .get(provider_type)
            .ok_or_else(|| anyhow::anyhow!("no adapter registered for provider {:?}", provider_type))
    }
}

#[async_trait]
impl InferenceProviderPort for ProviderRouter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        self.get(&job.provider_type)?.infer(job).await
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        match self.get(&job.provider_type) {
            Ok(provider) => provider.stream_tokens(job),
            Err(e) => Box::pin(async_stream::stream! {
                yield Err(e);
            }),
        }
    }
}

// ── Builder ────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ProviderRouterBuilder {
    providers: HashMap<ProviderType, Arc<dyn InferenceProviderPort>>,
}

impl ProviderRouterBuilder {
    pub fn register(
        mut self,
        provider_type: ProviderType,
        adapter: Arc<dyn InferenceProviderPort>,
    ) -> Self {
        self.providers.insert(provider_type, adapter);
        self
    }

    pub fn build(self) -> ProviderRouter {
        ProviderRouter {
            providers: self.providers,
        }
    }
}

// ── Dynamic provider router ────────────────────────────────────────────────────

/// Routes inference calls to providers registered in the database.
///
/// For Ollama: picks the server with the most available VRAM (via `/api/ps`).
/// For Gemini: picks the first active key (round-robin in future).
///
/// If no provider of the requested type is registered, the stream yields an error.
pub struct DynamicProviderRouter {
    registry: Arc<dyn LlmProviderRegistry>,
    model_selection_repo: Option<Arc<dyn ProviderModelSelectionRepository>>,
    ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
}

impl DynamicProviderRouter {
    pub fn new(registry: Arc<dyn LlmProviderRegistry>) -> Self {
        Self { registry, model_selection_repo: None, ollama_model_repo: None }
    }

    pub fn with_model_selection(
        mut self,
        repo: Arc<dyn ProviderModelSelectionRepository>,
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

    /// Select the best available provider for the given type.
    /// Returns the `LlmProvider` record so callers can build a specific adapter.
    pub async fn pick_provider(&self, pt: &ProviderType) -> Result<LlmProvider> {
        pick_best_provider(&*self.registry, None, self.model_selection_repo.as_deref(), self.ollama_model_repo.as_deref(), pt, "", None, None).await
    }
}

#[async_trait]
impl InferenceProviderPort for DynamicProviderRouter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let cfg = pick_best_provider(&*self.registry, None, self.model_selection_repo.as_deref(), self.ollama_model_repo.as_deref(), &job.provider_type, job.model_name.as_str(), None, None).await?;
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
            let cfg = match pick_best_provider(&*registry, None, model_selection_repo.as_deref(), ollama_model_repo.as_deref(), &job.provider_type, job.model_name.as_str(), None, None).await {
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

// ── Provider selection helpers ─────────────────────────────────────────────────

/// Filter providers by model selection: if a provider has selection rows for the
/// requested model and it is not enabled, skip that provider.
/// Providers with no selection rows or on repo error are kept (no restriction).
async fn filter_by_model_selection(
    candidates: Vec<LlmProvider>,
    repo: &dyn ProviderModelSelectionRepository,
    model_name: &str,
    provider_label: &str,
) -> Vec<LlmProvider> {
    let mut result = Vec::with_capacity(candidates.len());
    for b in candidates {
        match repo.list_enabled(b.id).await {
            Ok(enabled) if !enabled.is_empty() => {
                let set: HashSet<&str> = enabled.iter().map(|s| s.as_str()).collect();
                if set.contains(model_name) {
                    result.push(b);
                } else {
                    tracing::debug!(
                        provider_id = %b.id,
                        name = %b.name,
                        model_name = %model_name,
                        "model not enabled on {} provider, skipping", provider_label,
                    );
                }
            }
            // No rows or error → no restriction, include this provider.
            _ => result.push(b),
        }
    }
    result
}

// ── Gemini rate-limit helpers ──────────────────────────────────────────────────

/// Valkey key for per-(provider, model) RPM counter.
/// Bucketed by minute — TTL=120s so it always expires naturally.
fn gemini_rpm_key(provider_id: uuid::Uuid, model: &str) -> String {
    let minute = chrono::Utc::now().timestamp() / 60;
    valkey_keys::gemini_rpm(provider_id, model, minute)
}

/// Valkey key for per-(provider, model) RPD counter.
/// Bucketed by UTC date — TTL=90000s (~25h).
fn gemini_rpd_key(provider_id: uuid::Uuid, model: &str) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    valkey_keys::gemini_rpd(provider_id, model, &date)
}

/// Returns `(rpm_exhausted, rpd_exhausted)` for a given free-tier provider + model.
/// Both false when Valkey is unavailable (fail-open).
async fn gemini_limit_status(
    provider_id: uuid::Uuid,
    model: &str,
    rpm_limit: i32,
    rpd_limit: i32,
    valkey: &fred::clients::Pool,
) -> (bool, bool) {
    use fred::prelude::*;

    let rpm_exhausted = if rpm_limit > 0 {
        let count: i64 = valkey
            .get::<Option<i64>, _>(gemini_rpm_key(provider_id, model))
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        count >= rpm_limit as i64
    } else {
        false
    };

    let rpd_exhausted = if rpd_limit > 0 {
        let count: i64 = valkey
            .get::<Option<i64>, _>(gemini_rpd_key(provider_id, model))
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        count >= rpd_limit as i64
    } else {
        false
    };

    (rpm_exhausted, rpd_exhausted)
}

/// Increment per-(provider, model) RPM and RPD counters after a successful inference.
pub async fn increment_gemini_counters(
    pool: &fred::clients::Pool,
    provider_id: uuid::Uuid,
    model: &str,
) -> anyhow::Result<()> {
    use fred::prelude::*;

    let rpm_key = gemini_rpm_key(provider_id, model);
    let rpd_key = gemini_rpd_key(provider_id, model);

    let _: i64 = pool.incr_by(&rpm_key, 1).await?;
    let _: bool = pool.expire(&rpm_key, GEMINI_RPM_TTL_SECS, None).await?;

    let _: i64 = pool.incr_by(&rpd_key, 1).await?;
    let _: bool = pool.expire(&rpd_key, GEMINI_RPD_TTL_SECS, None).await?;

    Ok(())
}

/// Pick the best provider from the registry for the given type and model.
///
/// `tier_filter` restricts which Gemini providers are considered:
///   - `Some("free")` — only `is_free_tier=true` providers; no paid fallback.
///   - `None` (default) — auto: free-tier first, paid fallback when exhausted.
///
/// Gemini dispatch rules:
///   - Free-tier providers checked first (registration order).
///   - RPD exhausted → skip this provider for today.
///   - RPM exhausted but RPD ok → ALL free-tier providers are RPM-limited:
///     sleep until next minute boundary, then retry (up to MAX_RPM_RETRIES times).
///   - All free-tier RPD-exhausted → fall back to paid (unless tier_filter="free").
///   - Paid providers: if model_selection_repo has rows for the provider and the
///     requested model is NOT enabled, that paid provider is skipped.
///
/// Ollama: picks the server with the most available VRAM.
#[allow(clippy::too_many_arguments)]
pub async fn pick_best_provider(
    registry: &dyn LlmProviderRegistry,
    policy_repo: Option<&dyn GeminiPolicyRepository>,
    model_selection_repo: Option<&dyn ProviderModelSelectionRepository>,
    ollama_model_repo: Option<&dyn OllamaModelRepository>,
    pt: &ProviderType,
    model_name: &str,
    valkey: Option<&fred::clients::Pool>,
    tier_filter: Option<&str>,
) -> Result<LlmProvider> {
    let all = registry.list_all().await?;
    let candidates: Vec<LlmProvider> = all
        .into_iter()
        .filter(|b| b.is_active && &b.provider_type == pt)
        .collect();

    if candidates.is_empty() {
        return Err(anyhow::anyhow!(
            "no registered provider for {:?} — register one via POST /v1/providers",
            pt
        ));
    }

    match pt {
        ProviderType::Gemini => {
            pick_gemini_provider(candidates, policy_repo, model_selection_repo, model_name, valkey, tier_filter).await
        }

        ProviderType::Ollama => {
            // Filter to providers that have the requested model synced (if DB is populated).
            let filtered_candidates = if let Some(repo) = ollama_model_repo {
                if !model_name.is_empty() {
                    match repo.providers_for_model(model_name).await {
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

            // Filter by model selection: if a provider has selection rows for this model
            // and the model is disabled, skip that provider.
            let selection_filtered = if let Some(repo) = model_selection_repo {
                if !model_name.is_empty() {
                    filter_by_model_selection(filtered_candidates, repo, model_name, "ollama").await
                } else {
                    filtered_candidates
                }
            } else {
                filtered_candidates
            };

            let mut best: Option<(LlmProvider, i64)> = None;
            for b in selection_filtered {
                let avail = get_ollama_available_vram_mb(&b, valkey).await;
                match &best {
                    None => best = Some((b, avail)),
                    Some((_, v)) if avail > *v => best = Some((b, avail)),
                    _ => {}
                }
            }
            best.map(|(b, _)| b)
                .ok_or_else(|| anyhow::anyhow!("no Ollama provider with available VRAM"))
        }
    }
}

async fn pick_gemini_provider(
    candidates: Vec<LlmProvider>,
    policy_repo: Option<&dyn GeminiPolicyRepository>,
    model_selection_repo: Option<&dyn ProviderModelSelectionRepository>,
    model_name: &str,
    valkey: Option<&fred::clients::Pool>,
    tier_filter: Option<&str>,
) -> Result<LlmProvider> {
    // Look up the shared rate-limit policy for this model.
    let policy = if let Some(repo) = policy_repo {
        repo.get_for_model(model_name).await.unwrap_or(None)
    } else {
        None
    };
    let rpm_limit = policy.as_ref().map(|p| p.rpm_limit).unwrap_or(0);
    let rpd_limit = policy.as_ref().map(|p| p.rpd_limit).unwrap_or(0);
    let available_on_free_tier = policy.as_ref().map(|p| p.available_on_free_tier).unwrap_or(true);

    let (free_providers_all, raw_paid_providers_all): (Vec<_>, Vec<_>) =
        candidates.into_iter().partition(|b| b.is_free_tier);

    // Apply tier filter: restrict which pools are considered.
    let (free_providers, raw_paid_providers) = match tier_filter {
        Some("free") => (free_providers_all, Vec::new()),  // free only, no paid fallback
        _ => (free_providers_all, raw_paid_providers_all),  // auto: free-first, paid-fallback
    };

    // Filter paid providers by model selection: if a provider has selection rows
    // and the requested model is not enabled, skip that provider.
    let paid_providers = if let Some(repo) = model_selection_repo {
        filter_by_model_selection(raw_paid_providers, repo, model_name, "paid gemini").await
    } else {
        raw_paid_providers
    };

    // If the model is not available on free tier, skip free-tier providers entirely.
    if !available_on_free_tier {
        if tier_filter == Some("free") {
            return Err(anyhow::anyhow!(
                "model '{}' is not available on free tier (policy restriction)",
                model_name
            ));
        }
        if let Some(paid) = paid_providers.first() {
            tracing::info!(
                model_name = %model_name,
                provider_id = %paid.id,
                name = %paid.name,
                "model not available on free tier, routing directly to paid provider"
            );
            return Ok(paid.clone());
        }
        return Err(anyhow::anyhow!(
            "model '{}' requires a paid Gemini provider but none is configured",
            model_name
        ));
    }

    let Some(pool) = valkey else {
        // No Valkey → skip rate-limit checks entirely, use first free or paid.
        if let Some(b) = free_providers.first() {
            return Ok(b.clone());
        }
        if let Some(b) = paid_providers.first() {
            return Ok(b.clone());
        }
        return Err(anyhow::anyhow!("no active Gemini provider available"));
    };

    let mut all_rpd_exhausted = !free_providers.is_empty();

    for b in &free_providers {
        let (rpm_ex, rpd_ex) =
            gemini_limit_status(b.id, model_name, rpm_limit, rpd_limit, pool).await;

        if rpd_ex {
            tracing::info!(provider_id = %b.id, name = %b.name,
                "Gemini provider RPD exhausted for today, skipping");
            continue;
        }

        // This key still has daily quota.
        all_rpd_exhausted = false;

        if !rpm_ex {
            // Found a key with both RPM and RPD available.
            return Ok(b.clone());
        }
    }

    // All free keys are RPD-exhausted → fall back to paid.
    if all_rpd_exhausted {
        if let Some(paid) = paid_providers.first() {
            tracing::info!(provider_id = %paid.id, name = %paid.name,
                "all Gemini free-tier providers RPD-exhausted, using paid provider");
            return Ok(paid.clone());
        }
        return Err(anyhow::anyhow!(
            "all Gemini free-tier providers exhausted daily quota and no paid provider configured"
        ));
    }

    // All free-tier providers are RPM-limited (daily quota still available).
    // Fall back to paid immediately instead of sleeping in the request handler.
    if let Some(paid) = paid_providers.first() {
        tracing::info!(provider_id = %paid.id, name = %paid.name,
            "all Gemini free-tier providers RPM-limited, falling back to paid provider");
        return Ok(paid.clone());
    }

    // No paid fallback — return 429-friendly error immediately instead of
    // blocking the connection with tokio::time::sleep for up to 60s.
    let now_secs = chrono::Utc::now().timestamp();
    let secs_to_next_minute = 60 - (now_secs % 60);
    Err(anyhow::anyhow!(
        "all Gemini free-tier providers are RPM-limited; retry after ~{}s",
        secs_to_next_minute
    ))
}

/// Return available VRAM in MiB for an Ollama provider.
///
/// Priority:
/// 1. Valkey hardware metrics cache (set by health_checker when linked to a GpuServer).
///    Also enforces a temperature guard: providers at or above 85 °C are treated as
///    unavailable (returns `i64::MIN`).
/// 2. Live Ollama `/api/ps` poll (fallback when no agent data is cached).
/// 3. `i64::MAX` when `total_vram_mb == 0` (VRAM unknown → treat as unlimited).
/// 4. `0` on any network / parse error (treats provider as full).
pub async fn get_ollama_available_vram_mb(
    provider: &LlmProvider,
    valkey: Option<&fred::clients::Pool>,
) -> i64 {
    // ── 1. Valkey cache (agent data) ─────────────────────────────────────────
    if let Some(pool) = valkey
        && let Some(hw) = load_hw_metrics(pool, provider.id).await {
            if hw.is_overheating() {
                tracing::warn!(
                    provider_id = %provider.id,
                    name = %provider.name,
                    temp = hw.max_temp_c(),
                    "provider overheating — skipping dispatch"
                );
                return i64::MIN;
            }
            if hw.vram_total_mb > 0 {
                return hw.vram_free_mb();
            }
        }

    // ── 2. No agent data cached → assume full VRAM is available.
    // The health_checker refreshes the cache every ~30s. Between cache misses,
    // assume the provider has its registered VRAM (or unlimited if 0).
    if provider.total_vram_mb == 0 {
        return i64::MAX;
    }
    provider.total_vram_mb
}

/// Validate that a provider URL does not target known-dangerous internal services.
///
/// Blocks cloud metadata endpoints (169.254.169.254, metadata.google.internal),
/// Kubernetes internal services (.svc.cluster.local), and link-local IP addresses.
/// Localhost/private IPs are intentionally allowed since Ollama commonly runs there.
fn validate_provider_url(url_str: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url_str)
        .map_err(|_| anyhow::anyhow!("invalid provider URL"))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("provider URL has no host"))?;

    // Block cloud metadata services
    if host == "169.254.169.254" || host == "metadata.google.internal" {
        anyhow::bail!("metadata service URLs are not allowed as provider endpoints");
    }

    // Block Kubernetes internal services
    if host.contains(".svc.cluster.local") {
        anyhow::bail!("internal Kubernetes service URLs are not allowed as provider endpoints");
    }

    // Block link-local IP ranges (IPv4 169.254.0.0/16, IPv6 fe80::/10)
    if let Ok(ip) = host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_link_local() {
                    anyhow::bail!("link-local addresses are not allowed as provider endpoints");
                }
            }
            IpAddr::V6(v6) => {
                if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                    anyhow::bail!("IPv6 link-local addresses are not allowed as provider endpoints");
                }
            }
        }
    }

    Ok(())
}

/// Build a concrete inference adapter from a provider DB record.
///
/// Validates the provider URL against SSRF-dangerous targets before constructing
/// the adapter. Providers with blocked URLs are logged and return an error adapter
/// that yields a descriptive failure on every call.
pub fn make_adapter(cfg: &LlmProvider) -> Arc<dyn InferenceProviderPort> {
    match cfg.provider_type {
        ProviderType::Ollama => {
            if let Err(e) = validate_provider_url(&cfg.url) {
                tracing::warn!(
                    provider_id = %cfg.id,
                    name = %cfg.name,
                    url = %cfg.url,
                    "SSRF: skipping provider — {e}"
                );
                return Arc::new(BlockedAdapter(e.to_string()));
            }
            Arc::new(OllamaAdapter::new(&cfg.url))
        }
        ProviderType::Gemini => {
            // Gemini uses a fixed Google API host; URL validation is N/A.
            let key = cfg.api_key_encrypted.as_deref().unwrap_or("");
            Arc::new(GeminiAdapter::new(key))
        }
    }
}

/// Sentinel adapter returned when a provider's URL fails SSRF validation.
struct BlockedAdapter(String);

#[async_trait]
impl InferenceProviderPort for BlockedAdapter {
    async fn infer(&self, _job: &InferenceJob) -> Result<InferenceResult> {
        Err(anyhow::anyhow!("provider blocked: {}", self.0))
    }

    fn stream_tokens(
        &self,
        _job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let msg = self.0.clone();
        Box::pin(async_stream::stream! {
            yield Err(anyhow::anyhow!("provider blocked: {}", msg));
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::domain::entities::LlmProvider;
    use crate::domain::enums::LlmProviderStatus;
    use uuid::Uuid;

    fn make_provider(total_vram_mb: i64) -> LlmProvider {
        LlmProvider {
            id: Uuid::now_v7(),
            name: "test".into(),
            provider_type: crate::domain::enums::ProviderType::Ollama,
            url: "http://localhost:11434".into(),
            api_key_encrypted: None,
            is_active: true,
            total_vram_mb,
            gpu_index: None,
            server_id: None,
            is_free_tier: false,
            num_parallel: 4,
            status: LlmProviderStatus::Online,
            registered_at: chrono::Utc::now(),
        }
    }

    /// Graceful degradation: Valkey cache miss → static VRAM fallback.
    #[tokio::test]
    async fn vram_fallback_on_cache_miss() {
        let provider = make_provider(24576);
        // No Valkey connection → should return total_vram_mb as fallback
        let vram = get_ollama_available_vram_mb(&provider, None).await;
        assert_eq!(vram, 24576);
    }

    /// Graceful degradation: unknown VRAM (0) → unlimited (i64::MAX).
    #[tokio::test]
    async fn vram_unknown_returns_unlimited() {
        let provider = make_provider(0);
        let vram = get_ollama_available_vram_mb(&provider, None).await;
        assert_eq!(vram, i64::MAX);
    }

    #[test]
    fn validate_url_blocks_cloud_metadata() {
        assert!(validate_provider_url("http://169.254.169.254/latest/meta-data").is_err());
        assert!(validate_provider_url("http://metadata.google.internal").is_err());
    }

    #[test]
    fn validate_url_allows_localhost() {
        assert!(validate_provider_url("http://localhost:11434").is_ok());
        assert!(validate_provider_url("http://192.168.1.10:11434").is_ok());
    }
}
