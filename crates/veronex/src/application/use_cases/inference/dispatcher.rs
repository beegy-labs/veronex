use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::outbound::circuit_breaker_port::CircuitBreakerPort;
use crate::application::ports::outbound::concurrency_port::VramPoolPort;
use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::model_manager_port::ModelManagerPort;
use crate::application::ports::outbound::observability_port::ObservabilityPort;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::application::ports::outbound::provider_dispatch_port::ProviderDispatchPort;
use crate::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use crate::application::ports::outbound::thermal_port::ThermalPort;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::entities::{InferenceJob, LlmProvider};
use crate::domain::enums::{JobSource, KeyTier, ProviderType, ThrottleLevel};
use crate::domain::value_objects::JobStatusEvent;
use crate::domain::constants::{
    GEMINI_TIER_FREE, INITIAL_TOKEN_CAPACITY, JOB_OWNER_TTL_SECS,
    MODEL_LOCALITY_BONUS_MB, NO_PROVIDER_BACKOFF, QUEUE_ERROR_BACKOFF,
    QUEUE_JOBS as QUEUE_KEY_API, QUEUE_JOBS_PAID as QUEUE_KEY_API_PAID,
    QUEUE_JOBS_TEST as QUEUE_KEY_TEST, QUEUE_POLL_INTERVAL, QUEUE_PROCESSING,
};
use crate::application::ports::outbound::concurrency_port::VramPermit;

use super::JobEntry;
use super::runner::run_job;

// ── Provider filtering ──────────────────────────────────────────────────────

/// Three-stage filter: active+type+tier → model availability → model selection.
async fn filter_candidates(
    registry: &dyn LlmProviderRegistry,
    ollama_model_repo: &Option<Arc<dyn OllamaModelRepository>>,
    model_selection_repo: &Option<Arc<dyn ProviderModelSelectionRepository>>,
    provider_type: ProviderType,
    model: &str,
    gemini_tier: Option<&str>,
) -> Vec<LlmProvider> {
    let all = registry.list_all().await.unwrap_or_default();

    // Stage 1: active + type + tier
    let mut candidates: Vec<_> = all.into_iter()
        .filter(|b| {
            b.is_active && b.provider_type == provider_type
                && !matches!(gemini_tier, Some(GEMINI_TIER_FREE) if !b.is_free_tier)
        })
        .collect();

    if provider_type != ProviderType::Ollama || model.is_empty() {
        return candidates;
    }

    // Stage 2: model availability
    if let Some(repo) = ollama_model_repo
        && let Ok(ids) = repo.providers_for_model(model).await
            && !ids.is_empty() {
                let id_set: std::collections::HashSet<Uuid> = ids.into_iter().collect();
                let filtered: Vec<_> = candidates.iter()
                    .filter(|b| id_set.contains(&b.id)).cloned().collect();
                if !filtered.is_empty() { candidates = filtered; }
            }

    // Stage 3: model selection (disabled models)
    if let Some(repo) = model_selection_repo {
        let mut result = Vec::new();
        for b in candidates {
            match repo.list_enabled(b.id).await {
                Ok(enabled) if !enabled.is_empty() => {
                    if enabled.iter().any(|s| s == model) {
                        result.push(b);
                    } else {
                        tracing::debug!(provider_id = %b.id, %model, "model disabled, skipping");
                    }
                }
                _ => result.push(b),
            }
        }
        result
    } else {
        candidates
    }
}

/// Score by VRAM + locality bonus, sort by tier preference, claim first available slot.
#[allow(clippy::too_many_arguments)]
async fn score_and_claim(
    candidates: Vec<LlmProvider>,
    dispatch: &dyn ProviderDispatchPort,
    vram: &dyn VramPoolPort,
    thermal: &dyn ThermalPort,
    cb: &dyn CircuitBreakerPort,
    model: &str,
    key_tier: Option<&KeyTier>,
    provider_type: ProviderType,
) -> Option<(LlmProvider, VramPermit)> {
    let mut scored: Vec<(LlmProvider, i64)> = Vec::with_capacity(candidates.len());
    for b in candidates {
        let avail = match b.provider_type {
            ProviderType::Ollama => {
                let base = dispatch.available_vram_mb(&b).await;
                if vram.loaded_model_names(b.id).iter().any(|m| m == model) {
                    base.saturating_add(MODEL_LOCALITY_BONUS_MB)
                } else { base }
            }
            ProviderType::Gemini => i64::MAX,
        };
        scored.push((b, avail));
    }

    scored.sort_by(|a, b| {
        if provider_type == ProviderType::Ollama {
            let tier_pref = |p: &LlmProvider| match key_tier {
                Some(KeyTier::Paid) => !p.is_free_tier,
                Some(KeyTier::Free) => p.is_free_tier,
                None => false,
            };
            match tier_pref(&b.0).cmp(&tier_pref(&a.0)) {
                std::cmp::Ordering::Equal => b.1.cmp(&a.1),
                ord => ord,
            }
        } else { b.1.cmp(&a.1) }
    });

    scored.into_iter()
        .filter(|(_, avail)| *avail > 0)
        .find_map(|(provider, _)| {
            if !cb.is_allowed(provider.id) { return None; }
            match thermal.get_level(provider.id) {
                ThrottleLevel::Hard => return None,
                ThrottleLevel::Soft if vram.provider_active_requests(provider.id) > 0 => return None,
                _ => {}
            }
            vram.try_reserve(provider.id, model).map(|permit| (provider, permit))
        })
}

// ── Direct spawn (no-Valkey dev mode) ───────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_job_direct(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    job_repo: Arc<dyn JobRepository>,
    valkey: Option<Arc<dyn ValkeyPort>>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    vram_pool: Arc<dyn VramPoolPort>,
    thermal: Arc<dyn ThermalPort>,
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    uuid: Uuid,
    job: InferenceJob,
    gemini_tier: Option<String>,
    event_tx: broadcast::Sender<JobStatusEvent>,
    instance_id: Arc<str>,
    cancel_notifiers: Arc<DashMap<Uuid, Arc<Notify>>>,
) {
    tokio::spawn(async move {
        let (adapter, provider_id, is_free) = match provider_dispatch
            .pick_and_build(&job.provider_type, job.model_name.as_str(), gemini_tier.as_deref())
            .await
        {
            Ok(r) => r,
            Err(e) => { tracing::error!(job_id = %uuid, "no provider: {e}"); return; }
        };

        if !circuit_breaker.is_allowed(provider_id) {
            tracing::warn!(job_id = %uuid, "direct spawn skipped — circuit open");
            return;
        }
        match thermal.get_level(provider_id) {
            ThrottleLevel::Hard => { tracing::warn!(job_id = %uuid, "direct spawn skipped — hard throttle"); return; }
            ThrottleLevel::Soft if vram_pool.provider_active_requests(provider_id) > 0 => {
                tracing::debug!(job_id = %uuid, "direct spawn skipped — soft throttle"); return;
            }
            _ => {}
        }

        let permit = match vram_pool.try_reserve(provider_id, job.model_name.as_str()) {
            Some(p) => p,
            None => { tracing::warn!(job_id = %uuid, "direct spawn skipped — VRAM unavailable"); return; }
        };

        match run_job(
            jobs, adapter, job_repo, valkey, observability, model_manager,
            provider_dispatch, uuid, job, Some(provider_id), is_free,
            event_tx, instance_id, cancel_notifiers,
        ).await {
            Ok(()) => circuit_breaker.on_success(provider_id),
            Err(e) => {
                tracing::error!(job_id = %uuid, "inference job failed: {e}");
                circuit_breaker.on_failure(provider_id);
            }
        }
        drop(permit);
    });
}

// ── Queue dispatcher loop ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn queue_dispatcher_loop(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    registry: Arc<dyn LlmProviderRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey: Arc<dyn ValkeyPort>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    vram_pool: Arc<dyn VramPoolPort>,
    thermal: Arc<dyn ThermalPort>,
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    event_tx: broadcast::Sender<JobStatusEvent>,
    instance_id: Arc<str>,
    cancel_notifiers: Arc<DashMap<Uuid, Arc<Notify>>>,
    ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
    model_selection_repo: Option<Arc<dyn ProviderModelSelectionRepository>>,
    shutdown: CancellationToken,
) {
    tracing::info!("queue dispatcher started — priority: {QUEUE_KEY_API_PAID} > {QUEUE_KEY_API} > {QUEUE_KEY_TEST}");
    let source_queues: &[&str] = &[QUEUE_KEY_API_PAID, QUEUE_KEY_API, QUEUE_KEY_TEST];

    loop {
        let result = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            r = valkey.queue_priority_pop(source_queues, QUEUE_PROCESSING) => r,
        };

        let payload = match result {
            Ok(None) => { tokio::time::sleep(QUEUE_POLL_INTERVAL).await; continue; }
            Ok(Some(v)) => v,
            Err(e) => { tracing::error!("dispatcher pop error: {e}"); tokio::time::sleep(QUEUE_ERROR_BACKOFF).await; continue; }
        };

        let uuid = match Uuid::parse_str(&payload) {
            Ok(u) => u,
            Err(e) => { tracing::error!("invalid UUID '{payload}': {e}"); continue; }
        };

        // Load job from memory or DB
        let (job, gemini_tier, key_tier) = if let Some(entry) = jobs.get(&uuid) {
            (entry.job.clone(), entry.gemini_tier.clone(), entry.key_tier)
        } else {
            let job_id = crate::domain::value_objects::JobId(uuid);
            match job_repo.get(&job_id).await {
                Ok(Some(j)) => {
                    jobs.entry(uuid).or_insert_with(|| JobEntry {
                        job: j.clone(), status: j.status,
                        tokens: Vec::with_capacity(INITIAL_TOKEN_CAPACITY),
                        done: false, api_key_id: None,
                        notify: Arc::new(Notify::new()),
                        cancel_notify: Arc::new(Notify::new()),
                        gemini_tier: None, key_tier: None, tpm_reservation_minute: None,
                    });
                    (j, None, None)
                }
                Ok(None) => { tracing::warn!(%uuid, "queued job not in DB — skipping"); continue; }
                Err(e) => { tracing::error!(%uuid, "failed to load job: {e}"); continue; }
            }
        };

        // Find provider + claim VRAM
        let model = job.model_name.as_str();
        let candidates = filter_candidates(
            registry.as_ref(), &ollama_model_repo, &model_selection_repo,
            job.provider_type, model, gemini_tier.as_deref(),
        ).await;

        let claimed = score_and_claim(
            candidates, provider_dispatch.as_ref(), vram_pool.as_ref(),
            thermal.as_ref(), circuit_breaker.as_ref(), model,
            key_tier.as_ref(), job.provider_type,
        ).await;

        match claimed {
            Some((cfg, permit)) => {
                let pid = cfg.id;
                let is_free = cfg.is_free_tier;
                let adapter = provider_dispatch.build_adapter(&cfg);
                tracing::info!(%uuid, provider_id = %pid, name = %cfg.name, "dispatching");

                let owner_key = crate::domain::constants::job_owner_key(uuid);
                let _ = valkey.kv_set(&owner_key, instance_id.as_ref(), JOB_OWNER_TTL_SECS, false).await;

                let (jobs_c, repo_c, vk_c, obs_c, mm_c) = (
                    jobs.clone(), job_repo.clone(), valkey.clone(),
                    observability.clone(), model_manager.clone(),
                );
                let (ev_c, cb_c, pd_c, iid_c, cn_c) = (
                    event_tx.clone(), circuit_breaker.clone(), provider_dispatch.clone(),
                    instance_id.clone(), cancel_notifiers.clone(),
                );
                let uuid_str = uuid.to_string();

                tokio::spawn(async move {
                    let _permit = permit;
                    match run_job(
                        jobs_c, adapter, repo_c, Some(vk_c.clone()), obs_c, mm_c,
                        pd_c, uuid, job, Some(pid), is_free, ev_c, iid_c, cn_c,
                    ).await {
                        Ok(()) => cb_c.on_success(pid),
                        Err(e) => { tracing::error!(%uuid, %pid, "job failed: {e}"); cb_c.on_failure(pid); }
                    }
                    let _ = vk_c.list_remove(QUEUE_PROCESSING, &uuid_str).await;
                    let _ = vk_c.kv_del(&owner_key).await;
                });
            }
            None => {
                tracing::debug!(%uuid, "no provider, re-queuing");
                let uuid_str = uuid.to_string();
                let _ = valkey.list_remove(QUEUE_PROCESSING, &uuid_str).await;
                let requeue = match (job.source, key_tier) {
                    (JobSource::Test, _) => QUEUE_KEY_TEST,
                    (_, Some(KeyTier::Paid)) => QUEUE_KEY_API_PAID,
                    _ => QUEUE_KEY_API,
                };
                if let Err(e) = valkey.queue_push_front(requeue, uuid).await {
                    tracing::error!(%uuid, "re-queue failed: {e}");
                }
                tokio::time::sleep(NO_PROVIDER_BACKOFF).await;
            }
        }
    }

    tracing::info!("queue dispatcher stopped");
}
