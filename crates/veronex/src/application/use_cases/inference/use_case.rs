use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use futures::Stream;
use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::inbound::inference_use_case::{InferenceUseCase, SubmitJobRequest};
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
use crate::application::ports::outbound::thermal_drain_port::ThermalDrainPort;
use crate::application::ports::outbound::thermal_port::ThermalPort;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{JobSource, JobStatus, KeyTier};
use crate::domain::errors::DomainError;
use crate::domain::value_objects::{JobId, JobStatusEvent, ModelName, Prompt, StreamToken};
use crate::domain::constants::{
    INITIAL_TOKEN_CAPACITY,
    MAX_QUEUE_SIZE, MAX_QUEUE_PER_MODEL,
    TIER_BONUS_PAID, TIER_BONUS_STANDARD, TIER_BONUS_TEST,
};

use super::JobEntry;
use super::dispatcher::{queue_dispatcher_loop, spawn_job_direct};
use super::helpers::{broadcast_event, schedule_cleanup};
use super::runner::run_job;

type Result<T> = std::result::Result<T, DomainError>;

// ── UseCase struct ──────────────────────────────────────────────────────────

pub struct InferenceUseCaseImpl {
    registry: Arc<dyn LlmProviderRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey: Option<Arc<dyn ValkeyPort>>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    vram_pool: Arc<dyn VramPoolPort>,
    thermal: Arc<dyn ThermalPort>,
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    event_tx: broadcast::Sender<JobStatusEvent>,
    message_store: Option<Arc<dyn MessageStore>>,
    ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
    model_selection_repo: Option<Arc<dyn ProviderModelSelectionRepository>>,
    instance_id: Arc<str>,
    cancel_notifiers: Arc<DashMap<Uuid, Arc<Notify>>>,
}

impl InferenceUseCaseImpl {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry: Arc<dyn LlmProviderRegistry>,
        job_repo: Arc<dyn JobRepository>,
        valkey: Option<Arc<dyn ValkeyPort>>,
        observability: Option<Arc<dyn ObservabilityPort>>,
        model_manager: Option<Arc<dyn ModelManagerPort>>,
        vram_pool: Arc<dyn VramPoolPort>,
        thermal: Arc<dyn ThermalPort>,
        circuit_breaker: Arc<dyn CircuitBreakerPort>,
        provider_dispatch: Arc<dyn ProviderDispatchPort>,
        event_tx: broadcast::Sender<JobStatusEvent>,
        message_store: Option<Arc<dyn MessageStore>>,
        ollama_model_repo: Option<Arc<dyn OllamaModelRepository>>,
        model_selection_repo: Option<Arc<dyn ProviderModelSelectionRepository>>,
        instance_id: Arc<str>,
    ) -> Self {
        Self {
            registry, job_repo, valkey, observability, model_manager,
            jobs: Arc::new(DashMap::new()),
            vram_pool, thermal, circuit_breaker, provider_dispatch,
            event_tx, message_store, ollama_model_repo, model_selection_repo,
            instance_id, cancel_notifiers: Arc::new(DashMap::new()),
        }
    }

    pub fn cancel_notifiers(&self) -> Arc<DashMap<Uuid, Arc<Notify>>> {
        self.cancel_notifiers.clone()
    }

    pub fn as_thermal_drain(&self) -> Arc<dyn ThermalDrainPort> {
        Arc::new(ThermalDrainAdapter { jobs: self.jobs.clone() })
    }
}

struct ThermalDrainAdapter {
    jobs: Arc<DashMap<Uuid, JobEntry>>,
}

impl ThermalDrainPort for ThermalDrainAdapter {
    fn cancel_jobs_for_provider(&self, provider_id: Uuid) -> usize {
        self.jobs.iter()
            .filter(|e| e.assigned_provider_id == Some(provider_id))
            .map(|e| { e.cancel_notify.notify_one(); 1 })
            .sum()
    }
}

impl InferenceUseCaseImpl {

    pub fn start_job_sweeper(
        &self, shutdown: CancellationToken,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        let jobs = self.jobs.clone();
        let cn = self.cancel_notifiers.clone();
        async move {
            const INTERVAL: std::time::Duration = crate::domain::constants::PENDING_JOB_SWEEP_INTERVAL;
            const MAX_AGE: chrono::Duration = chrono::Duration::minutes(10);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = tokio::time::sleep(INTERVAL) => {}
                }
                let now = chrono::Utc::now();
                let stale: Vec<Uuid> = jobs.iter()
                    .filter(|e| e.status == JobStatus::Pending && now.signed_duration_since(e.job.created_at) > MAX_AGE)
                    .map(|e| *e.key())
                    .collect();
                let swept = stale.len();
                for id in stale { jobs.remove(&id); cn.remove(&id); }
                if swept > 0 { tracing::info!(swept, "sweeper: removed stale entries"); }
            }
        }
    }

    pub fn start_queue_worker(
        &self, shutdown: CancellationToken,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        use futures::FutureExt as _;
        let Some(ref valkey) = self.valkey else {
            return futures::future::ready(()).boxed();
        };
        let (jobs, registry, job_repo, valkey) = (
            self.jobs.clone(), self.registry.clone(),
            self.job_repo.clone(), valkey.clone(),
        );
        let (obs, mm, vram, thermal, cb, pd) = (
            self.observability.clone(), self.model_manager.clone(),
            self.vram_pool.clone(), self.thermal.clone(),
            self.circuit_breaker.clone(), self.provider_dispatch.clone(),
        );
        let (ev, iid, cn, omr, msr) = (
            self.event_tx.clone(), self.instance_id.clone(),
            self.cancel_notifiers.clone(), self.ollama_model_repo.clone(),
            self.model_selection_repo.clone(),
        );
        tracing::info!("multi-provider queue dispatcher started");
        async move {
            queue_dispatcher_loop(
                jobs, registry, job_repo, valkey, obs, mm, vram, thermal,
                cb, pd, ev, iid, cn, omr, msr, shutdown,
            ).await;
        }.boxed()
    }

    pub async fn recover_pending_jobs(&self) -> anyhow::Result<()> {
        let Some(ref valkey) = self.valkey else { return Ok(()); };
        let pending = self.job_repo.list_pending().await?;
        if pending.is_empty() { return Ok(()); }

        tracing::info!("recovering {} pending/running jobs", pending.len());
        for mut job in pending {
            let uuid = job.id.0;
            if job.status == JobStatus::Running {
                // Check if another node currently owns this job — skip if so.
                let owner_key = crate::domain::constants::job_owner_key(uuid);
                if let Ok(Some(owner)) = valkey.kv_get(&owner_key).await
                    && owner != self.instance_id.as_ref()
                {
                    tracing::info!(
                        %uuid, current_owner = %owner,
                        "skipping recovery — job owned by another node"
                    );
                    continue;
                }
                if let Err(e) = self.job_repo.update_status(&job.id, JobStatus::Pending).await {
                    tracing::warn!(%uuid, "failed to reset running→pending: {e}");
                }
                job.status = JobStatus::Pending;
                job.started_at = None;
            }
            self.jobs.entry(uuid).or_insert_with(|| JobEntry {
                job: job.clone(), status: job.status,
                tokens: Vec::with_capacity(INITIAL_TOKEN_CAPACITY),
                done: false, api_key_id: job.api_key_id,
                notify: Arc::new(Notify::new()),
                cancel_notify: Arc::new(Notify::new()),
                gemini_tier: None, key_tier: None, tpm_reservation_minute: None,
                assigned_provider_id: None,
            });
            // Re-enqueue to ZSET with emergency priority (recovered jobs get highest priority)
            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            let score = (now_ms.saturating_sub(TIER_BONUS_PAID)) as f64; // max priority for recovery
            let model = job.model_name.as_str();
            match valkey.zset_enqueue(uuid, score, model, now_ms, MAX_QUEUE_SIZE, MAX_QUEUE_PER_MODEL).await {
                Ok(true) => tracing::info!(%uuid, "recovered job re-enqueued to ZSET"),
                Ok(false) => tracing::warn!(%uuid, "ZSET full during recovery"),
                Err(e) => tracing::warn!(%uuid, "failed to re-enqueue to ZSET: {e}"),
            }
        }
        Ok(())
    }
}

// ── InferenceUseCase trait impl ─────────────────────────────────────────────

#[async_trait]
impl InferenceUseCase for InferenceUseCaseImpl {
    async fn submit(&self, req: SubmitJobRequest) -> Result<JobId> {
        let SubmitJobRequest {
            prompt, model_name, provider_type, gemini_tier, api_key_id,
            account_id, source, api_format, messages, tools, request_path,
            conversation_id, key_tier, images,
        } = req;

        let job_id = JobId::new();
        let job = InferenceJob {
            id: job_id.clone(),
            prompt: Prompt::new(&prompt)?,
            model_name: ModelName::new(&model_name)?,
            status: JobStatus::Pending, provider_type,
            created_at: chrono::Utc::now(),
            started_at: None, completed_at: None, error: None, result_text: None,
            api_key_id, account_id,
            latency_ms: None, ttft_ms: None, prompt_tokens: None,
            completion_tokens: None, cached_tokens: None,
            source, provider_id: None, api_format, messages, tools,
            request_path, queue_time_ms: None, cancelled_at: None,
            conversation_id, tool_calls_json: None,
            messages_hash: None, messages_prefix_hash: None, failure_reason: None,
            images,
        };

        // Upload messages to S3
        if let (Some(msgs), Some(store)) = (&job.messages, &self.message_store)
            && let Err(e) = store.put(job_id.0, msgs).await {
                tracing::warn!(job_id = %job_id.0, "S3 upload failed (non-fatal): {e}");
            }
        let job_for_db = InferenceJob { messages: None, ..job.clone() };
        self.job_repo.save(&job_for_db).await?;

        let cancel_notify = Arc::new(Notify::new());
        self.cancel_notifiers.insert(job_id.0, cancel_notify.clone());
        self.jobs.insert(job_id.0, JobEntry {
            job: job.clone(), status: JobStatus::Pending,
            tokens: Vec::with_capacity(INITIAL_TOKEN_CAPACITY),
            done: false, api_key_id,
            notify: Arc::new(Notify::new()), cancel_notify,
            gemini_tier: gemini_tier.clone(), key_tier,
            tpm_reservation_minute: Some(chrono::Utc::now().timestamp() / 60),
            assigned_provider_id: None,
        });

        let uuid = job_id.0;
        broadcast_event(&self.event_tx, &self.valkey, &self.instance_id, &JobStatusEvent {
            id: uuid.to_string(), status: "pending".into(),
            model_name: job.model_name.as_str().into(),
            provider_type: job.provider_type.as_str().into(), latency_ms: None,
        }).await;

        // Compute ZSET score: now_ms - tier_bonus (lower = higher priority)
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let tier_bonus = match (source, key_tier) {
            (JobSource::Test, _) => TIER_BONUS_TEST,
            (_, Some(KeyTier::Paid)) => TIER_BONUS_PAID,
            _ => TIER_BONUS_STANDARD,
        };
        let score = (now_ms.saturating_sub(tier_bonus)) as f64;

        if let Some(ref valkey) = self.valkey {
            match valkey.zset_enqueue(uuid, score, &model_name, now_ms, MAX_QUEUE_SIZE, MAX_QUEUE_PER_MODEL).await {
                Ok(true) => tracing::debug!(%uuid, %score, "job enqueued to ZSET"),
                Ok(false) => {
                    // Queue full — mark DB job failed (orphan prevention) then reject.
                    tracing::warn!(%uuid, "queue full, rejecting job");
                    self.jobs.remove(&uuid);
                    self.cancel_notifiers.remove(&uuid);
                    if let Err(e) = self.job_repo.fail_with_reason(
                        &job_id, "queue_full", Some("queue capacity exceeded"),
                    ).await {
                        tracing::warn!(%uuid, "failed to mark queue-full job as failed: {e}");
                    }
                    return Err(DomainError::QueueFull("queue capacity exceeded".into()));
                }
                Err(e) => {
                    tracing::warn!(%uuid, "Valkey ZSET enqueue failed, direct spawn: {e}");
                    spawn_job_direct(
                        self.jobs.clone(), self.job_repo.clone(), self.valkey.clone(),
                        self.observability.clone(), self.model_manager.clone(),
                        self.vram_pool.clone(), self.thermal.clone(),
                        self.circuit_breaker.clone(), self.provider_dispatch.clone(),
                        uuid, job, gemini_tier, self.event_tx.clone(),
                        self.instance_id.clone(), self.cancel_notifiers.clone(),
                    );
                }
            }
        } else {
            spawn_job_direct(
                self.jobs.clone(), self.job_repo.clone(), None,
                self.observability.clone(), self.model_manager.clone(),
                self.vram_pool.clone(), self.thermal.clone(),
                self.circuit_breaker.clone(), self.provider_dispatch.clone(),
                uuid, job, gemini_tier, self.event_tx.clone(),
                self.instance_id.clone(), self.cancel_notifiers.clone(),
            );
        }

        Ok(job_id)
    }

    async fn process(&self, job_id: &JobId) -> Result<()> {
        let uuid = job_id.0;
        let (job, gemini_tier) = {
            let entry = self.jobs.get(&uuid)
                .ok_or_else(|| anyhow::anyhow!("job not found: {uuid}"))?;
            if matches!(entry.status, JobStatus::Running | JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled) {
                return Ok(());
            }
            (entry.job.clone(), entry.gemini_tier.clone())
        };

        let (adapter, pid, is_free) = self.provider_dispatch
            .pick_and_build(&job.provider_type, job.model_name.as_str(), gemini_tier.as_deref())
            .await?;

        run_job(
            self.jobs.clone(), adapter, self.job_repo.clone(), self.valkey.clone(),
            self.observability.clone(), self.model_manager.clone(),
            self.provider_dispatch.clone(), uuid, job, Some(pid), is_free,
            self.event_tx.clone(), self.instance_id.clone(), self.cancel_notifiers.clone(),
        ).await?;
        Ok(())
    }

    fn stream(&self, job_id: &JobId) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let jobs = self.jobs.clone();
        let job_repo = self.job_repo.clone();
        let uuid = job_id.0;

        Box::pin(async_stream::try_stream! {
            if !jobs.contains_key(&uuid) {
                let jid = JobId(uuid);
                match job_repo.get(&jid).await? {
                    Some(j) if j.status == JobStatus::Completed => {
                        if let Some(text) = j.result_text
                            && !text.is_empty() { yield StreamToken::text(text); }
                        yield StreamToken::done();
                        return;
                    }
                    Some(j) if j.status == JobStatus::Failed => {
                        Err(anyhow::anyhow!("{}", j.error.unwrap_or_else(|| "inference failed".into())))?;
                        return;
                    }
                    Some(_) => { Err(anyhow::anyhow!("job not in memory: {uuid}"))?; return; }
                    None => { Err(anyhow::anyhow!("job not found: {uuid}"))?; return; }
                }
            }

            let mut idx: usize = 0;
            loop {
                let (new_tokens, done, notify) = {
                    let entry = jobs.get(&uuid)
                        .ok_or_else(|| anyhow::anyhow!("job entry disappeared: {uuid}"))?;
                    (entry.tokens[idx..].to_vec(), entry.done, entry.notify.clone())
                };
                for token in new_tokens { idx += 1; yield token; }
                if done { break; }
                notify.notified().await;
            }
        })
    }

    async fn get_status(&self, job_id: &JobId) -> Result<JobStatus> {
        if let Some(entry) = self.jobs.get(&job_id.0) {
            return Ok(entry.status);
        }
        let job = self.job_repo.get(job_id).await?
            .ok_or_else(|| anyhow::anyhow!("job not found: {}", job_id))?;
        Ok(job.status)
    }

    async fn cancel(&self, job_id: &JobId) -> Result<()> {
        let is_local = self.jobs.contains_key(&job_id.0);

        let is_final = if let Some(mut entry) = self.jobs.get_mut(&job_id.0) {
            if matches!(entry.status, JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled) {
                true
            } else {
                entry.status = JobStatus::Cancelled;
                entry.done = true;
                let (n, cn) = (entry.notify.clone(), entry.cancel_notify.clone());
                drop(entry);
                n.notify_one();
                cn.notify_one();
                false
            }
        } else { false };

        if !is_final {
            self.job_repo.cancel_job(job_id, chrono::Utc::now()).await?;

            // Try to remove from ZSET queue (if still queued, before dispatch)
            if let Some(ref vk) = self.valkey {
                let model = self.jobs.get(&job_id.0)
                    .map(|e| e.job.model_name.as_str().to_string())
                    .unwrap_or_default();
                let _ = vk.zset_cancel(&job_id.0.to_string(), &model).await;
            }
        }
        if !is_local
            && let Some(ref vk) = self.valkey { vk.publish_cancel(job_id.0).await; }
        self.cancel_notifiers.remove(&job_id.0);
        // Schedule deferred removal from in-memory job map (queued jobs never reach runner cleanup).
        if is_local && !is_final {
            schedule_cleanup(&self.jobs, job_id.0, crate::domain::constants::JOB_CLEANUP_DELAY);
        }
        Ok(())
    }
}
