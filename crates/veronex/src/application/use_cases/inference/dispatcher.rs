use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::outbound::circuit_breaker_port::CircuitBreakerPort;
use crate::application::ports::outbound::concurrency_port::VramPoolPort;
use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::message_store::MessageStore;
use crate::application::ports::outbound::model_manager_port::ModelManagerPort;
use crate::application::ports::outbound::observability_port::ObservabilityPort;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::application::ports::outbound::provider_dispatch_port::ProviderDispatchPort;
use crate::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use crate::application::ports::outbound::global_model_settings::GlobalModelSettingsRepository;
use crate::application::ports::outbound::thermal_port::ThermalPort;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::entities::{InferenceJob, LlmProvider};
use crate::domain::enums::{JobStatus, KeyTier, ProviderType, ThrottleLevel};
use crate::domain::value_objects::JobStatusEvent;
use crate::domain::constants::{
    GEMINI_TIER_FREE, INITIAL_TOKEN_CAPACITY, JOB_CLEANUP_DELAY, JOB_OWNER_TTL_SECS,
    MODEL_LOCALITY_BONUS_MB, NO_PROVIDER_BACKOFF, QUEUE_ERROR_BACKOFF,
    QUEUE_POLL_INTERVAL, QUEUE_PROCESSING,
    LOCALITY_BONUS_MS, ZSET_PEEK_K, ZSET_PEEK_K_MAX,
};
use crate::application::ports::outbound::concurrency_port::VramPermit;

use super::JobEntry;
use super::helpers::{decr_pending, schedule_cleanup};
use super::runner::run_job;

// ── Provider filtering ──────────────────────────────────────────────────────

/// Five-stage filter: global model gate → active+type+tier → model availability → model selection → preload exclusion.
async fn filter_candidates(
    registry: &dyn LlmProviderRegistry,
    ollama_model_repo: &Option<Arc<dyn OllamaModelRepository>>,
    model_selection_repo: &Option<Arc<dyn ProviderModelSelectionRepository>>,
    global_model_settings_repo: &Option<Arc<dyn GlobalModelSettingsRepository>>,
    vram_pool: &dyn VramPoolPort,
    provider_type: ProviderType,
    model: &str,
    gemini_tier: Option<&str>,
) -> Vec<LlmProvider> {
    // Stage 0: global model gate — if model is globally disabled, reject immediately
    if !model.is_empty() {
        if let Some(repo) = global_model_settings_repo {
            if let Ok(enabled) = repo.is_enabled(model).await {
                if !enabled {
                    tracing::debug!(model, "model globally disabled — rejecting all providers");
                    return vec![];
                }
            }
        }
    }

    let all = registry.list_all().await.unwrap_or_default();

    // Stage 1: active + type + tier (standby providers included — woken on demand)
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

    // Stage 3: model selection (disabled models) — parallel lookups
    if let Some(repo) = model_selection_repo {
        let futs: Vec<_> = candidates.iter()
            .map(|b| {
                let id = b.id;
                async move { (id, repo.list_enabled(id).await) }
            })
            .collect();
        let results = futures::future::join_all(futs).await;
        let mut filtered = Vec::with_capacity(candidates.len());
        for (b, (_, res)) in candidates.into_iter().zip(results) {
            match res {
                Ok(enabled) if !enabled.is_empty() => {
                    if enabled.iter().any(|s| s == model) {
                        filtered.push(b);
                    } else {
                        tracing::debug!(provider_id = %b.id, %model, "model disabled, skipping");
                    }
                }
                _ => filtered.push(b),
            }
        }
        candidates = filtered;
    }

    // Stage 4: preload exclusion (Phase 6) — skip model+provider pairs
    // with 3 consecutive preload failures within the 300s exclusion window.
    if provider_type == ProviderType::Ollama && !model.is_empty() {
        let before = candidates.len();
        candidates.retain(|b| !vram_pool.is_preload_excluded(b.id, model));
        let excluded = before - candidates.len();
        if excluded > 0 {
            tracing::debug!(%model, excluded, "providers excluded due to preload failures");
        }
    }

    candidates
}

/// Maximum candidates to score — bounds the scoring loop at scale.
const MAX_SCORING_CANDIDATES: usize = 50;

/// Score by VRAM + locality bonus, sort by tier preference, claim first available slot.
/// All operations are O(1) atomic reads — no async I/O needed.
fn score_and_claim(
    mut candidates: Vec<LlmProvider>,
    vram: &dyn VramPoolPort,
    thermal: &dyn ThermalPort,
    cb: &dyn CircuitBreakerPort,
    model: &str,
    key_tier: Option<&KeyTier>,
    provider_type: ProviderType,
) -> Option<(LlmProvider, VramPermit)> {
    // Cap candidates to avoid O(N) scoring at 10K+ providers
    if candidates.len() > MAX_SCORING_CANDIDATES {
        candidates.truncate(MAX_SCORING_CANDIDATES);
    }
    let mut scored: Vec<(LlmProvider, i64)> = Vec::with_capacity(candidates.len());
    for b in candidates {
        let avail = match b.provider_type {
            ProviderType::Ollama => {
                // Use VramPool's O(1) atomic read instead of per-provider Valkey call.
                // Thermal/overheating checks are handled below in the find_map closure.
                let base = vram.available_vram_mb(b.id) as i64;
                // VramPool returns 0 when agent hasn't pushed capacity yet.
                // total_vram_mb = 0 means unlimited (server handles capacity internally).
                // Treat as max available so dispatcher never blocks on unknown VRAM.
                let base = if base == 0 { i64::MAX / 2 } else { base };
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
                ThrottleLevel::Hard | ThrottleLevel::Cooldown => return None,
                ThrottleLevel::Soft if vram.provider_active_requests(provider.id) > 0 => return None,
                ThrottleLevel::RampUp => {
                    // Phase 8: Cooldown ramp-up — force max_concurrent=1 during RampUp.
                    // AIMD loop will gradually increase back to normal.
                    let current_mc = vram.max_concurrent(provider.id, model);
                    if current_mc > 1 {
                        // Save pre-Hard snapshot if not already saved
                        if vram.pre_hard_max_concurrent(provider.id, model) == 0 {
                            vram.set_pre_hard_max_concurrent(provider.id, model, current_mc);
                        }
                        vram.set_max_concurrent(provider.id, model, 1);
                    }
                }
                _ => {}
            }
            // Wake standby provider on demand (instant Scale-Out recovery)
            if vram.is_standby(provider.id) {
                vram.set_standby(provider.id, false);
                tracing::info!(provider_id = %provider.id, %model, "dispatch: woke standby provider on demand");
            }
            vram.try_reserve(provider.id, model).map(|permit| (provider, permit))
        })
}

// ── Fail job when no provider is available ────────────────────────────────────

async fn fail_job_no_provider(
    jobs: &Arc<DashMap<Uuid, JobEntry>>,
    job_repo: &Arc<dyn JobRepository>,
    valkey: &Option<Arc<dyn ValkeyPort>>,
    uuid: Uuid,
    reason: &str,
) {
    // pending → failed: DECR pending
    decr_pending(valkey).await;
    if let Some(mut entry) = jobs.get_mut(&uuid) {
        entry.status = JobStatus::Failed;
        entry.job.status = JobStatus::Failed;
        entry.job.error = Some(reason.to_string());
        entry.job.failure_reason = Some("no_eligible_provider".to_string());
        entry.done = true;
        let notify = entry.notify.clone();
        drop(entry);
        notify.notify_one();
    }

    let job_id = crate::domain::value_objects::JobId(uuid);
    if let Err(e) = job_repo.fail_with_reason(&job_id, "no_eligible_provider", Some(reason)).await {
        tracing::warn!(job_id = %uuid, "failed to persist no-provider failure: {e}");
    }

    schedule_cleanup(jobs, uuid, JOB_CLEANUP_DELAY);
}

// ── Direct spawn (no-Valkey dev mode) ───────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_job_direct(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    job_repo: Arc<dyn JobRepository>,
    message_store: Option<Arc<dyn MessageStore>>,
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
            Err(e) => {
                tracing::error!(job_id = %uuid, "no provider: {e}");
                fail_job_no_provider(&jobs, &job_repo, &valkey, uuid, &e.to_string()).await;
                return;
            }
        };

        if !circuit_breaker.is_allowed(provider_id) {
            tracing::warn!(job_id = %uuid, "direct spawn skipped — circuit open");
            return;
        }
        match thermal.get_level(provider_id) {
            ThrottleLevel::Hard | ThrottleLevel::Cooldown => { tracing::warn!(job_id = %uuid, "direct spawn skipped — hard/cooldown throttle"); return; }
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
            jobs, adapter, job_repo, message_store, valkey, observability, model_manager,
            provider_dispatch, uuid, job, Some(provider_id), is_free,
            event_tx, instance_id, cancel_notifiers,
        ).await {
            Ok(Some(latency_ms)) => {
                circuit_breaker.on_success(provider_id);
                circuit_breaker.record_latency(provider_id, latency_ms as u64);
            }
            Ok(None) => {} // cancelled or ownership lost
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
    message_store: Option<Arc<dyn MessageStore>>,
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
    global_model_settings_repo: Option<Arc<dyn GlobalModelSettingsRepository>>,
    shutdown: CancellationToken,
) {
    tracing::info!("queue dispatcher started — ZSET scoring (locality + age × perf_factor)");

    loop {
        // ── 1. Peek top-K from ZSET ─────────────────────────────────────
        let peek_result = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            r = valkey.zset_peek(adaptive_k(&valkey).await) => r,
        };

        let candidates_raw = match peek_result {
            Ok(v) if v.is_empty() => { tokio::time::sleep(QUEUE_POLL_INTERVAL).await; continue; }
            Ok(v) => v,
            Err(e) => { tracing::error!("dispatcher ZSET peek error: {e}"); tokio::time::sleep(QUEUE_ERROR_BACKOFF).await; continue; }
        };

        let now_ms = chrono::Utc::now().timestamp_millis() as f64;

        // ── 2. Score each candidate in Rust ─────────────────────────────
        // Load job metadata, compute final_score = zset_score - locality_bonus - age_bonus
        let mut scored: Vec<(String, f64, crate::domain::entities::InferenceJob, Option<String>, Option<KeyTier>)> = Vec::new();

        for (job_id_str, zset_score) in &candidates_raw {
            let uuid = match Uuid::parse_str(job_id_str) {
                Ok(u) => u,
                Err(e) => { tracing::error!("invalid UUID '{job_id_str}': {e}"); continue; }
            };

            // Load job from memory or DB
            let (job, gemini_tier, key_tier) = if let Some(entry) = jobs.get(&uuid) {
                (entry.job.clone(), entry.gemini_tier.clone(), entry.key_tier)
            } else {
                let jid = crate::domain::value_objects::JobId(uuid);
                match job_repo.get(&jid).await {
                    Ok(Some(j)) => {
                        jobs.entry(uuid).or_insert_with(|| JobEntry {
                            job: j.clone(), status: j.status,
                            tokens: Vec::with_capacity(INITIAL_TOKEN_CAPACITY),
                            done: false, api_key_id: None,
                            notify: Arc::new(Notify::new()),
                            cancel_notify: Arc::new(Notify::new()),
                            gemini_tier: None, key_tier: None, tpm_reservation_minute: None,
                            assigned_provider_id: None,
                            vision_analysis: None,
                        });
                        (j, None, None)
                    }
                    Ok(None) => {
                        tracing::warn!(%uuid, "queued job not in DB — removing from ZSET");
                        let _ = valkey.zset_cancel(job_id_str, "").await;
                        continue;
                    }
                    Err(e) => { tracing::error!(%uuid, "failed to load job: {e}"); continue; }
                }
            };

            let model = job.model_name.as_str();

            // Locality bonus: model already loaded on some provider?
            let locality = if vram_pool.is_model_loaded(model) {
                LOCALITY_BONUS_MS
            } else {
                0.0
            };

            // Age bonus: wait_ms × 0.25 × perf_factor
            let wait_ms = (now_ms - zset_score).max(0.0);
            let pf = thermal.global_perf_factor();
            let age = wait_ms * 0.25 * pf as f64;

            let final_score = zset_score - locality - age;

            scored.push((job_id_str.clone(), final_score, job, gemini_tier, key_tier));
        }

        if scored.is_empty() {
            tokio::time::sleep(QUEUE_POLL_INTERVAL).await;
            continue;
        }

        // Sort by final_score ascending (lowest = highest priority)
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // ── 3. Try to claim + dispatch the best candidate ───────────────
        let mut dispatched = false;

        for (job_id_str, _final_score, job, gemini_tier, key_tier) in scored {
            let uuid = Uuid::parse_str(&job_id_str).expect("already validated");
            let model = job.model_name.as_str();

            // Find provider + claim VRAM
            let candidates = filter_candidates(
                registry.as_ref(), &ollama_model_repo, &model_selection_repo,
                &global_model_settings_repo,
                vram_pool.as_ref(), job.provider_type, model, gemini_tier.as_deref(),
            ).await;

            if candidates.is_empty() {
                // No eligible provider → atomically remove from ZSET and fail
                let claimed = valkey.zset_claim(&job_id_str, QUEUE_PROCESSING, model).await.unwrap_or(false);
                if claimed {
                    let _ = valkey.list_remove(QUEUE_PROCESSING, &job_id_str).await;
                    let vk_opt: Option<Arc<dyn ValkeyPort>> = Some(valkey.clone());
                    fail_job_no_provider(&jobs, &job_repo, &vk_opt, uuid, "no eligible provider for this model").await;
                    dispatched = true;
                }
                continue;
            }

            let claimed_provider = score_and_claim(
                candidates, vram_pool.as_ref(),
                thermal.as_ref(), circuit_breaker.as_ref(), model,
                key_tier.as_ref(), job.provider_type,
            );

            let Some((cfg, permit)) = claimed_provider else {
                // Provider busy — skip this job, try next in window
                continue;
            };

            // Atomic ZSET claim (ZREM + RPUSH processing + DECR demand)
            match valkey.zset_claim(&job_id_str, QUEUE_PROCESSING, model).await {
                Ok(true) => { /* claimed successfully */ }
                Ok(false) => {
                    // Another instance already took it — release VRAM and try next
                    drop(permit);
                    continue;
                }
                Err(e) => {
                    tracing::error!(%uuid, "ZSET claim error: {e}");
                    drop(permit);
                    continue;
                }
            }

            let pid = cfg.id;
            let is_free = cfg.is_free_tier;
            let adapter = provider_dispatch.build_adapter(&cfg);
            tracing::info!(%uuid, provider_id = %pid, name = %cfg.name, "dispatching");
            if let Some(mut e) = jobs.get_mut(&uuid) {
                e.assigned_provider_id = Some(pid);
            }

            let owner_key = crate::domain::constants::job_owner_key(uuid);
            let _ = valkey.kv_set(&owner_key, instance_id.as_ref(), JOB_OWNER_TTL_SECS, false).await;

            let (jobs_c, repo_c, ms_c, vk_c, obs_c, mm_c) = (
                jobs.clone(), job_repo.clone(), message_store.clone(),
                valkey.clone(), observability.clone(), model_manager.clone(),
            );
            let (ev_c, cb_c, pd_c, iid_c, cn_c) = (
                event_tx.clone(), circuit_breaker.clone(), provider_dispatch.clone(),
                instance_id.clone(), cancel_notifiers.clone(),
            );

            tokio::spawn(async move {
                let _permit = permit;
                match run_job(
                    jobs_c, adapter, repo_c, ms_c, Some(vk_c.clone()), obs_c, mm_c,
                    pd_c, uuid, job, Some(pid), is_free, ev_c, iid_c, cn_c,
                ).await {
                    Ok(Some(latency_ms)) => {
                        cb_c.on_success(pid);
                        cb_c.record_latency(pid, latency_ms as u64);
                    }
                    Ok(None) => {} // cancelled or ownership lost
                    Err(e) => { tracing::error!(%uuid, %pid, "job failed: {e}"); cb_c.on_failure(pid); }
                }
                let _ = vk_c.list_remove(QUEUE_PROCESSING, &job_id_str).await;
                let _ = vk_c.kv_del(&owner_key).await;
            });

            dispatched = true;
            break;
        }

        if !dispatched {
            // All candidates in window had no available provider slots
            tokio::time::sleep(NO_PROVIDER_BACKOFF).await;
        }
    }

    tracing::info!("queue dispatcher stopped");
}

/// Adaptive K: scale window size based on ZSET length.
/// K = min(ZSET_size / 3, ZSET_PEEK_K_MAX), floor at ZSET_PEEK_K.
async fn adaptive_k(valkey: &Arc<dyn ValkeyPort>) -> u64 {
    match valkey.zset_len().await {
        Ok(len) if len > ZSET_PEEK_K * 3 => (len / 3).min(ZSET_PEEK_K_MAX),
        _ => ZSET_PEEK_K,
    }
}
