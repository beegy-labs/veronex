use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::Stream;
use futures::StreamExt as _;
use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::inference_backend::InferenceBackendPort;
use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::message_store::MessageStore;
use crate::application::ports::outbound::model_manager_port::ModelManagerPort;
use crate::application::ports::outbound::provider_dispatch_port::ProviderDispatchPort;
use crate::application::ports::outbound::circuit_breaker_port::CircuitBreakerPort;
use crate::application::ports::outbound::concurrency_port::ConcurrencyPort;
use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};
use crate::application::ports::outbound::thermal_port::ThermalPort;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{ApiFormat, ProviderType, FinishReason, JobSource, JobStatus, ThrottleLevel};
use crate::domain::value_objects::{JobId, JobStatusEvent, ModelName, Prompt, StreamToken};

// ── Queue keys ─────────────────────────────────────────────────────────────────

/// Paid-tier API-client jobs — highest priority (BLPOP polled first).
const QUEUE_KEY_API_PAID: &str = "veronex:queue:jobs:paid";
/// Standard / free-tier API-client jobs — medium priority (BLPOP polled second).
const QUEUE_KEY_API: &str = "veronex:queue:jobs";
/// Test-panel jobs — lowest priority (BLPOP polled third).
const QUEUE_KEY_TEST: &str = "veronex:queue:jobs:test";

// ── In-memory job store ────────────────────────────────────────────────────────

struct JobEntry {
    job: InferenceJob,
    status: JobStatus,
    tokens: Vec<StreamToken>,
    done: bool,
    /// API key that submitted this job — used for TPM accounting.
    api_key_id: Option<Uuid>,
    notify: Arc<Notify>,
    /// Fired by cancel() to interrupt the Ollama token stream immediately.
    cancel_notify: Arc<Notify>,
    /// Gemini tier routing preference: "free" = free-tier only, None = auto (free→paid fallback).
    gemini_tier: Option<String>,
    /// API key billing tier: `Some("paid")` → QUEUE_KEY_API_PAID; `None`/`Some("free")` → QUEUE_KEY_API.
    /// Lost on server restart — recovered jobs fall back to the standard queue.
    key_tier: Option<String>,
}

// ── Use-case implementation ────────────────────────────────────────────────────

pub struct InferenceUseCaseImpl {
    /// Registry of all registered providers (Ollama servers, Gemini keys).
    /// Used by queue_dispatcher_loop for VRAM-aware routing.
    registry: Arc<dyn LlmProviderRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: Option<fred::clients::Pool>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    /// DashMap: 64 independent shard RwLocks — different UUIDs never contend.
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    /// Dynamic concurrency control — VRAM-aware semaphores per (provider, model).
    /// Updated by the capacity analyzer every 5 minutes.
    slot_map: Arc<dyn ConcurrencyPort>,
    /// Thermal throttle state — updated by health_checker every 30 s.
    thermal: Arc<dyn ThermalPort>,
    /// Per-provider circuit breaker — isolates providers after consecutive failures.
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    /// Provider selection, adapter construction, and rate-limit counter management.
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    /// Broadcast channel: fires on every job status transition (pending/running/completed/failed).
    /// Capacity 256 — slow consumers lag-skip rather than block producers.
    event_tx: broadcast::Sender<JobStatusEvent>,
    /// S3-compatible object store for conversation contexts. When set, messages_json
    /// is uploaded to S3 on submit() and DB column stays NULL for new jobs.
    message_store: Option<Arc<dyn MessageStore>>,
}

impl InferenceUseCaseImpl {
    pub fn new(
        registry: Arc<dyn LlmProviderRegistry>,
        job_repo: Arc<dyn JobRepository>,
        valkey_pool: Option<fred::clients::Pool>,
        observability: Option<Arc<dyn ObservabilityPort>>,
        model_manager: Option<Arc<dyn ModelManagerPort>>,
        slot_map: Arc<dyn ConcurrencyPort>,
        thermal: Arc<dyn ThermalPort>,
        circuit_breaker: Arc<dyn CircuitBreakerPort>,
        provider_dispatch: Arc<dyn ProviderDispatchPort>,
        event_tx: broadcast::Sender<JobStatusEvent>,
        message_store: Option<Arc<dyn MessageStore>>,
    ) -> Self {
        Self {
            registry,
            job_repo,
            valkey_pool,
            observability,
            model_manager,
            jobs: Arc::new(DashMap::new()),
            slot_map,
            thermal,
            circuit_breaker,
            provider_dispatch,
            event_tx,
            message_store,
        }
    }

    /// Spawn the multi-backend queue dispatcher (no-op if Valkey is not configured).
    ///
    /// The dispatcher pops jobs from the Valkey queue, finds the backend with the most
    /// available VRAM (via Ollama's `/api/ps`), and spawns each job concurrently.
    /// Each physical GPU (Ollama server) processes one job at a time; multiple GPUs
    /// run in parallel. If no backend has capacity, the job is re-queued and the
    /// dispatcher backs off briefly.
    pub fn start_queue_worker(
        &self,
        shutdown: CancellationToken,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        use futures::FutureExt as _;

        let Some(ref pool) = self.valkey_pool else {
            return futures::future::ready(()).boxed();
        };

        let jobs = self.jobs.clone();
        let registry = self.registry.clone();
        let job_repo = self.job_repo.clone();
        let valkey_pool = pool.clone();
        let observability = self.observability.clone();
        let model_manager = self.model_manager.clone();
        let slot_map = self.slot_map.clone();
        let thermal = self.thermal.clone();
        let circuit_breaker = self.circuit_breaker.clone();
        let provider_dispatch = self.provider_dispatch.clone();
        let event_tx = self.event_tx.clone();

        tracing::info!("multi-provider queue dispatcher started (VRAM-aware routing)");

        async move {
            queue_dispatcher_loop(
                jobs,
                registry,
                job_repo,
                valkey_pool,
                observability,
                model_manager,
                slot_map,
                thermal,
                circuit_breaker,
                provider_dispatch,
                event_tx,
                shutdown,
            )
            .await;
        }
        .boxed()
    }

    /// Re-enqueue jobs that were Pending or Running when the server last stopped.
    ///
    /// Running jobs are reset to Pending so they start fresh (in-flight token streams
    /// were lost on restart).  No-op when Valkey is not configured.
    pub async fn recover_pending_jobs(&self) -> anyhow::Result<()> {
        let Some(ref pool) = self.valkey_pool else {
            return Ok(());
        };

        let jobs_list = self.job_repo.list_pending().await?;
        if jobs_list.is_empty() {
            return Ok(());
        }

        tracing::info!("recovering {} pending/running jobs", jobs_list.len());

        use fred::prelude::*;
        for mut job in jobs_list {
            let uuid = job.id.0;

            // Reset interrupted Running jobs → Pending so they replay cleanly.
            if job.status == JobStatus::Running {
                if let Err(e) = self
                    .job_repo
                    .update_status(&job.id, JobStatus::Pending)
                    .await
                {
                    tracing::warn!(%uuid, "failed to reset running job to pending: {e}");
                }
                job.status = JobStatus::Pending;
                job.started_at = None;
            }

            self.jobs.entry(uuid).or_insert_with(|| JobEntry {
                job: job.clone(),
                status: job.status,
                tokens: Vec::with_capacity(256),
                done: false,
                api_key_id: None,
                notify: Arc::new(Notify::new()),
                cancel_notify: Arc::new(Notify::new()),
                gemini_tier: None, // tier preference is lost on restart → auto-routing
                key_tier: None,    // tier preference is lost on restart → standard queue
            });

            let queue_key = if job.source == JobSource::Test { QUEUE_KEY_TEST } else { QUEUE_KEY_API };
            if let Err(e) = pool.rpush::<i64, _, _>(queue_key, uuid.to_string()).await {
                tracing::warn!(%uuid, "failed to re-enqueue recovered job: {e}");
            } else {
                tracing::info!(%uuid, "recovered job re-enqueued");
            }
        }

        Ok(())
    }
}

#[async_trait]
impl InferenceUseCase for InferenceUseCaseImpl {
    async fn submit(
        &self,
        prompt: &str,
        model_name: &str,
        backend_type: &str,
        api_key_id: Option<Uuid>,
        account_id: Option<Uuid>,
        source: JobSource,
        api_format: ApiFormat,
        messages: Option<serde_json::Value>,
        tools: Option<serde_json::Value>,
        request_path: Option<String>,
        conversation_id: Option<String>,
        key_tier: Option<String>,
    ) -> Result<JobId> {
        let job_id = JobId::new();
        // Parse provider string: "gemini-free" routes to free-tier Gemini only;
        // "gemini" uses auto-routing (free-first, paid-fallback).
        let (provider_type, gemini_tier) = match backend_type {
            "gemini-free" => (ProviderType::Gemini, Some("free".to_string())),
            "gemini" => (ProviderType::Gemini, None),
            _ => (ProviderType::Ollama, None),
        };

        let job = InferenceJob {
            id: job_id.clone(),
            prompt: Prompt::new(prompt)?,
            model_name: ModelName::new(model_name)?,
            status: JobStatus::Pending,
            provider_type,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
            result_text: None,
            api_key_id,
            account_id,
            latency_ms: None,
            ttft_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            source,
            provider_id: None,
            api_format,
            messages,
            tools,
            request_path,
            queue_time_ms: None,
            cancelled_at: None,
            conversation_id,
            tool_calls_json: None,
        };

        // Upload conversation context to S3 (authoritative store for messages_json).
        // DB column stays NULL for new jobs — avoids JSONB bloat on inference_jobs.
        if let (Some(msgs), Some(store)) = (&job.messages, &self.message_store) {
            if let Err(e) = store.put(job_id.0, msgs).await {
                tracing::warn!(job_id = %job_id.0, "S3 message upload failed (non-fatal): {e}");
            }
        }
        // Save to DB without messages — COALESCE keeps NULL for new rows
        let job_for_db = InferenceJob { messages: None, ..job.clone() };
        self.job_repo.save(&job_for_db).await?;

        self.jobs.insert(
            job_id.0,
            JobEntry {
                job: job.clone(), // original: retains messages for dispatch
                status: JobStatus::Pending,
                tokens: Vec::with_capacity(256),
                done: false,
                api_key_id,
                notify: Arc::new(Notify::new()),
                cancel_notify: Arc::new(Notify::new()),
                gemini_tier: gemini_tier.clone(),
                key_tier: key_tier.clone(),
            },
        );

        let uuid = job_id.0;

        // Broadcast enqueue event before job is moved — network flow UI picks this up immediately.
        let _ = self.event_tx.send(JobStatusEvent {
            id: uuid.to_string(),
            status: "pending".to_string(),
            model_name: job.model_name.as_str().to_string(),
            provider_type: match job.provider_type { ProviderType::Ollama => "ollama", ProviderType::Gemini => "gemini" }.to_string(),
            latency_ms: None,
        });

        if let Some(ref pool) = self.valkey_pool {
            // Persistent queue: RPUSH job UUID — dispatcher picks it up.
            // Priority order: paid-tier API > free/standard API > test.
            use fred::prelude::*;
            let queue_key = if source == JobSource::Test {
                QUEUE_KEY_TEST
            } else if key_tier.as_deref() == Some("paid") {
                QUEUE_KEY_API_PAID
            } else {
                QUEUE_KEY_API
            };
            match pool.rpush::<i64, _, _>(queue_key, uuid.to_string()).await {
                Ok(_) => {
                    tracing::debug!(%uuid, "job enqueued to Valkey queue");
                }
                Err(e) => {
                    tracing::warn!(%uuid, "Valkey enqueue failed, falling back to direct spawn: {e}");
                    spawn_job_direct(
                        self.jobs.clone(),
                        self.job_repo.clone(),
                        self.valkey_pool.clone(),
                        self.observability.clone(),
                        self.model_manager.clone(),
                        self.slot_map.clone(),
                        self.thermal.clone(),
                        self.circuit_breaker.clone(),
                        self.provider_dispatch.clone(),
                        uuid,
                        job,
                        gemini_tier,
                        self.event_tx.clone(),
                    );
                }
            }
        } else {
            // No Valkey — immediate spawn (dev mode, direct registry dispatch).
            spawn_job_direct(
                self.jobs.clone(),
                self.job_repo.clone(),
                None,
                self.observability.clone(),
                self.model_manager.clone(),
                self.slot_map.clone(),
                self.thermal.clone(),
                self.circuit_breaker.clone(),
                self.provider_dispatch.clone(),
                uuid,
                job,
                gemini_tier,
                self.event_tx.clone(),
            );
        }

        Ok(job_id)
    }

    async fn process(&self, job_id: &JobId) -> Result<()> {
        let uuid = job_id.0;
        let (job, api_key_id, gemini_tier) = {
            let entry = self.jobs
                .get(&uuid)
                .ok_or_else(|| anyhow::anyhow!("job not found: {uuid}"))?;

            if matches!(
                entry.status,
                JobStatus::Running | JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
            ) {
                return Ok(());
            }

            (entry.job.clone(), entry.api_key_id, entry.gemini_tier.clone())
            // Ref dropped here — before any await
        };
        let _ = api_key_id; // used in spawned path; process() ignores it

        // For process(), pick the best available provider now.
        let (adapter, provider_id, provider_is_free_tier) = match self
            .provider_dispatch
            .pick_and_build(&job.provider_type, job.model_name.as_str(), gemini_tier.as_deref())
            .await
        {
            Ok(result) => result,
            Err(e) => return Err(e),
        };

        run_job(
            self.jobs.clone(),
            adapter,
            self.job_repo.clone(),
            self.valkey_pool.clone(),
            self.observability.clone(),
            self.model_manager.clone(),
            self.provider_dispatch.clone(),
            uuid,
            job,
            Some(provider_id),
            provider_is_free_tier,
            self.event_tx.clone(),
        )
        .await
    }

    fn stream(&self, job_id: &JobId) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let jobs = self.jobs.clone();
        let job_repo = self.job_repo.clone();
        let uuid = job_id.0;

        Box::pin(async_stream::try_stream! {
            // Fast-path: job is in the in-memory store (same process run).
            let in_memory = jobs.contains_key(&uuid);

            if !in_memory {
                // DB fallback: replay stored result for completed jobs that were
                // processed before the last server restart.
                let jid = JobId(uuid);
                match job_repo.get(&jid).await? {
                    Some(job) if job.status == JobStatus::Completed => {
                        if let Some(text) = job.result_text {
                            if !text.is_empty() {
                                yield StreamToken { value: text, is_final: false, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None };
                            }
                        }
                        yield StreamToken { value: String::new(), is_final: true, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None };
                        return;
                    }
                    Some(job) if job.status == JobStatus::Failed => {
                        let msg = job.error.unwrap_or_else(|| "inference failed".to_string());
                        Err(anyhow::anyhow!("{msg}"))?;
                        return;
                    }
                    Some(_) => {
                        // Pending/running but not in memory — should not normally happen.
                        Err(anyhow::anyhow!("job not in memory: {uuid}"))?;
                        return;
                    }
                    None => {
                        Err(anyhow::anyhow!("job not found: {uuid}"))?;
                        return;
                    }
                }
            }

            // In-memory streaming path.
            let mut idx: usize = 0;
            loop {
                let (new_tokens, done, notify) = {
                    let entry = jobs
                        .get(&uuid)
                        .ok_or_else(|| anyhow::anyhow!("job entry disappeared: {uuid}"))?;

                    let new_tokens = entry.tokens[idx..].to_vec();
                    let done = entry.done;
                    let notify = entry.notify.clone();
                    (new_tokens, done, notify)
                    // Ref dropped here — before yield/await
                };

                for token in new_tokens {
                    idx += 1;
                    yield token;
                }

                if done {
                    break;
                }

                notify.notified().await;
            }
        })
    }

    async fn get_status(&self, job_id: &JobId) -> Result<JobStatus> {
        // Fast path: in-memory.
        if let Some(entry) = self.jobs.get(&job_id.0) {
            return Ok(entry.status);
        }
        // Fallback: database (jobs from a previous server run).
        let job = self
            .job_repo
            .get(job_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("job not found: {}", job_id))?;
        Ok(job.status)
    }

    async fn cancel(&self, job_id: &JobId) -> Result<()> {
        let is_already_final = if let Some(mut entry) = self.jobs.get_mut(&job_id.0) {
            // Don't override a job that has already reached a terminal state.
            // This prevents a tab-close cleanup from flipping a completed job
            // to cancelled after the stream has naturally finished.
            if entry.status == JobStatus::Completed || entry.status == JobStatus::Failed {
                true
            } else {
                entry.status = JobStatus::Cancelled;
                entry.done = true;
                let notify = entry.notify.clone();
                let cancel_notify = entry.cancel_notify.clone();
                drop(entry); // drop RefMut before calling notify
                notify.notify_one();
                // Wake up run_job's tokio::select! so the stream is dropped immediately.
                cancel_notify.notify_one();
                false
            }
        } else {
            false
        };

        if !is_already_final {
            self.job_repo
                .cancel_job(job_id, chrono::Utc::now())
                .await?;
        }
        Ok(())
    }
}

// ── Direct spawn helper (no-Valkey dev mode) ──────────────────────────────────

fn spawn_job_direct(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: Option<fred::clients::Pool>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    slot_map: Arc<dyn ConcurrencyPort>,
    thermal: Arc<dyn ThermalPort>,
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    uuid: Uuid,
    job: InferenceJob,
    gemini_tier: Option<String>,
    event_tx: broadcast::Sender<JobStatusEvent>,
) {
    tokio::spawn(async move {
        let (adapter, provider_id, provider_is_free_tier) = match provider_dispatch
            .pick_and_build(&job.provider_type, job.model_name.as_str(), gemini_tier.as_deref())
            .await
        {
            Ok(result) => result,
            Err(e) => {
                tracing::error!(job_id = %uuid, "no provider available: {e}");
                return;
            }
        };

        // Circuit breaker gate — skip open providers even in direct mode.
        if !circuit_breaker.is_allowed(provider_id) {
            tracing::warn!(job_id = %uuid, %provider_id, "direct spawn skipped — circuit open");
            return;
        }

        // Respect thermal limits even in direct mode
        match thermal.get_level(provider_id) {
            ThrottleLevel::Hard => {
                tracing::warn!(job_id = %uuid, %provider_id, "direct spawn skipped — hard throttle");
                return;
            }
            ThrottleLevel::Soft => {
                if slot_map.active_slots(provider_id, job.model_name.as_str()) > 0 {
                    tracing::debug!(job_id = %uuid, "direct spawn skipped — soft throttle, already busy");
                    return;
                }
            }
            ThrottleLevel::Normal => {}
        }

        let permit = slot_map.try_acquire(provider_id, job.model_name.as_str());

        match run_job(
            jobs,
            adapter,
            job_repo,
            valkey_pool,
            observability,
            model_manager,
            provider_dispatch,
            uuid,
            job,
            Some(provider_id),
            provider_is_free_tier,
            event_tx,
        )
        .await
        {
            Ok(()) => circuit_breaker.on_success(provider_id),
            Err(e) => {
                tracing::error!(job_id = %uuid, "inference job failed: {e}");
                circuit_breaker.on_failure(provider_id);
            }
        }
        drop(permit); // RAII: slot auto-released
    });
}

// ── Multi-backend queue dispatcher ────────────────────────────────────────────

/// Pops jobs from the Valkey queue and dispatches each one to the best available
/// backend concurrently.
///
/// For each popped job:
///   1. Find the Ollama server with the most available VRAM (or first Gemini key).
///   2. If a backend is available and not currently busy: mark it busy, spawn the job.
///   3. If no backend is available: LPUSH the job back to the front of the queue and
///      back off briefly (2s) before retrying.
///
/// This allows N Ollama GPUs to work in parallel while each GPU processes one job
/// at a time (max_jobs = 1 per physical GPU).
#[allow(clippy::too_many_arguments)]
async fn queue_dispatcher_loop(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    registry: Arc<dyn LlmProviderRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: fred::clients::Pool,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    slot_map: Arc<dyn ConcurrencyPort>,
    thermal: Arc<dyn ThermalPort>,
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    event_tx: broadcast::Sender<JobStatusEvent>,
    shutdown: CancellationToken,
) {
    use fred::prelude::*;

    tracing::info!(
        "queue dispatcher loop started — priority: {QUEUE_KEY_API_PAID} > {QUEUE_KEY_API} > {QUEUE_KEY_TEST}"
    );

    loop {
        // BLPOP blocks for up to 5 s; returns None on timeout.
        // Priority order: paid-API > standard-API > test (BLPOP checks keys in order).
        let queue_keys: Vec<String> = vec![
            QUEUE_KEY_API_PAID.to_string(),
            QUEUE_KEY_API.to_string(),
            QUEUE_KEY_TEST.to_string(),
        ];
        let result: Result<Option<(String, String)>, _> = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            r = valkey_pool.blpop(queue_keys, 5.0) => r,
        };

        let payload = match result {
            Ok(None) => continue,
            Ok(Some((_key, value))) => value,
            Err(e) if matches!(e.kind(), fred::error::ErrorKind::Timeout) => continue,
            Err(e) => {
                tracing::error!("dispatcher BLPOP error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let uuid = match uuid::Uuid::parse_str(&payload) {
            Ok(u) => u,
            Err(e) => {
                tracing::error!("invalid UUID in queue payload '{payload}': {e}");
                continue;
            }
        };

        // Retrieve job from in-memory store (fast path) or DB (recovery path).
        // Also read gemini_tier and key_tier for routing.
        // Ref is held only in this block and dropped before the await below.
        let (job, gemini_tier, key_tier) = if let Some(entry) = jobs.get(&uuid) {
            (entry.job.clone(), entry.gemini_tier.clone(), entry.key_tier.clone())
            // Ref dropped here
        } else {
            let job_id = crate::domain::value_objects::JobId(uuid);
            match job_repo.get(&job_id).await {
                Ok(Some(j)) => {
                    jobs.entry(uuid).or_insert_with(|| JobEntry {
                        job: j.clone(),
                        status: j.status,
                        tokens: Vec::with_capacity(256),
                        done: false,
                        api_key_id: None,
                        notify: Arc::new(Notify::new()),
                        cancel_notify: Arc::new(Notify::new()),
                        gemini_tier: None,
                        key_tier: None, // tier lost on restart; recovered jobs use standard queue
                    });
                    (j, None, None)
                }
                Ok(None) => {
                    tracing::warn!(%uuid, "queued job not found in DB — skipping");
                    continue;
                }
                Err(e) => {
                    tracing::error!(%uuid, "failed to load job from DB: {e}");
                    continue;
                }
            }
        };

        // ── Find an available provider (VRAM check) and claim it atomically ──
        let provider_cfg = registry.list_all().await.unwrap_or_default();
        let candidates: Vec<_> = provider_cfg
            .into_iter()
            .filter(|b| {
                b.is_active && b.provider_type == job.provider_type
                    && match gemini_tier.as_deref() {
                        Some("free") => b.is_free_tier,
                        _ => true,
                    }
            })
            .collect();

        // Collect VRAM availability for each candidate.
        let mut availability: Vec<(crate::domain::entities::LlmProvider, i64)> = Vec::new();
        for b in candidates {
            let avail = match b.provider_type {
                ProviderType::Ollama => provider_dispatch.available_vram_mb(&b).await,
                ProviderType::Gemini => i64::MAX, // no VRAM constraint
            };
            availability.push((b, avail));
        }
        // Sort providers: primary = tier preference, secondary = available VRAM descending.
        // Paid-tier jobs prefer non-free-tier Ollama providers; free-tier jobs prefer free-tier ones.
        availability.sort_by(|a, b| {
            if job.provider_type == ProviderType::Ollama {
                let a_preferred = match key_tier.as_deref() {
                    Some("paid") => !a.0.is_free_tier, // paid → prefer non-free-tier
                    Some("free") => a.0.is_free_tier,  // free → prefer free-tier
                    _ => false,
                };
                let b_preferred = match key_tier.as_deref() {
                    Some("paid") => !b.0.is_free_tier,
                    Some("free") => b.0.is_free_tier,
                    _ => false,
                };
                match b_preferred.cmp(&a_preferred) {
                    std::cmp::Ordering::Equal => b.1.cmp(&a.1),
                    ord => ord,
                }
            } else {
                b.1.cmp(&a.1) // Gemini: VRAM = MAX for all, ordering doesn't matter
            }
        });

        // Claim a slot on the best available backend (VRAM-sorted, thermal-filtered,
        // circuit-breaker-filtered).
        let claimed = availability
            .into_iter()
            .filter(|(_b, avail)| *avail > 0)
            .find_map(|(backend, _)| {
                // Circuit breaker gate — skip open backends.
                if !circuit_breaker.is_allowed(backend.id) {
                    tracing::debug!(
                        provider_id = %backend.id,
                        provider_name = %backend.name,
                        "circuit open — skipping provider"
                    );
                    return None;
                }
                // Thermal gate
                match thermal.get_level(backend.id) {
                    ThrottleLevel::Hard => return None,
                    ThrottleLevel::Soft => {
                        // Soft throttle: allow only if no active slots (cap=1 effect)
                        if slot_map.active_slots(backend.id, job.model_name.as_str()) > 0 {
                            return None;
                        }
                    }
                    ThrottleLevel::Normal => {}
                }
                // Non-blocking semaphore acquire
                slot_map
                    .try_acquire(backend.id, job.model_name.as_str())
                    .map(|permit| (backend, permit))
            });

        match claimed {
            Some((provider_cfg, permit)) => {
                let provider_id = provider_cfg.id;
                let provider_is_free_tier = provider_cfg.is_free_tier;
                let adapter = provider_dispatch.build_adapter(&provider_cfg);

                tracing::info!(
                    %uuid,
                    provider_id = %provider_id,
                    provider_name = %provider_cfg.name,
                    "dispatching job to provider"
                );

                let jobs_c = jobs.clone();
                let job_repo_c = job_repo.clone();
                let valkey_c = valkey_pool.clone();
                let obs_c = observability.clone();
                let mm_c = model_manager.clone();
                let ev_c = event_tx.clone();
                let cb_c = circuit_breaker.clone();
                let pd_c = provider_dispatch.clone();

                tokio::spawn(async move {
                    let _permit = permit; // RAII: dropped when task finishes
                    match run_job(
                        jobs_c,
                        adapter,
                        job_repo_c,
                        Some(valkey_c),
                        obs_c,
                        mm_c,
                        pd_c,
                        uuid,
                        job,
                        Some(provider_id),
                        provider_is_free_tier,
                        ev_c,
                    )
                    .await
                    {
                        Ok(()) => cb_c.on_success(provider_id),
                        Err(e) => {
                            tracing::error!(%uuid, %provider_id, "inference job failed: {e}");
                            cb_c.on_failure(provider_id);
                        }
                    }
                    tracing::debug!(%provider_id, "slot released");
                });
            }
            None => {
                // No provider available → put job back at front of its original queue and wait.
                tracing::debug!(%uuid, "no available provider, re-queuing");
                let requeue_key = if job.source == JobSource::Test {
                    QUEUE_KEY_TEST
                } else if key_tier.as_deref() == Some("paid") {
                    QUEUE_KEY_API_PAID
                } else {
                    QUEUE_KEY_API
                };
                if let Err(e) = valkey_pool
                    .lpush::<i64, _, _>(requeue_key, uuid.to_string())
                    .await
                {
                    tracing::error!(%uuid, "failed to re-queue job: {e}");
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }

    tracing::info!("queue dispatcher stopped");
}

// ── Background job runner ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_job(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    backend: Arc<dyn InferenceBackendPort>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: Option<fred::clients::Pool>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    uuid: Uuid,
    mut job: InferenceJob,
    provider_id: Option<Uuid>,
    provider_is_free_tier: bool,
    event_tx: broadcast::Sender<JobStatusEvent>,
) -> Result<()> {
    // ── Model manager: ensure model is loaded (Ollama only) ──────────
    if job.provider_type == ProviderType::Ollama {
        if let Some(ref mm) = model_manager {
            if let Err(e) = mm.ensure_loaded(job.model_name.as_str()).await {
                tracing::warn!(%uuid, "model manager ensure_loaded failed (non-fatal): {e}");
            }
        }
    }

    // ── Running ──────────────────────────────────────────────────────
    let started_at = chrono::Utc::now();
    let api_key_id = if let Some(mut entry) = jobs.get_mut(&uuid) {
        if entry.status == JobStatus::Cancelled {
            return Ok(());
        }
        entry.status = JobStatus::Running;
        entry.job.status = JobStatus::Running;
        entry.job.started_at = Some(started_at);
        entry.api_key_id
        // RefMut dropped here — before the await below
    } else {
        None
    };

    job.status = JobStatus::Running;
    job.started_at = Some(started_at);
    job.provider_id = provider_id;
    // Record queue wait time: created_at → started_at
    job.queue_time_ms = Some(
        started_at
            .signed_duration_since(job.created_at)
            .num_milliseconds()
            .max(0) as i32,
    );
    if let Err(e) = job_repo.save(&job).await {
        tracing::warn!(job_id = %uuid, "failed to persist running state: {e}");
    }

    let _ = event_tx.send(JobStatusEvent {
        id: uuid.to_string(),
        status: "running".to_string(),
        model_name: job.model_name.as_str().to_string(),
        provider_type: match job.provider_type { ProviderType::Ollama => "ollama", ProviderType::Gemini => "gemini" }.to_string(),
        latency_ms: None,
    });

    // ── Stream tokens ────────────────────────────────────────────────
    // Clone cancel_notify before entering the loop so we can select! on it
    // without holding the jobs lock across an await.
    let cancel_notify = jobs
        .get(&uuid)
        .map(|e| e.cancel_notify.clone())
        .unwrap_or_else(|| Arc::new(Notify::new()));
    // Ref dropped here

    let mut token_stream = backend.stream_tokens(&job);
    // Clear messages after stream is created — S3 is authoritative; DB saves must omit
    job.messages = None;
    let mut token_count: u64 = 0;
    let mut accumulated_text = String::new();
    // Collected tool calls from all StreamToken.tool_calls across this job.
    // Stored as JSONB in inference_jobs.tool_calls_json for training data / dashboard.
    let mut accumulated_tool_calls: Vec<serde_json::Value> = Vec::new();
    // Actual token counts from backend usage metadata (e.g. Gemini usageMetadata).
    // Set when the final StreamToken carries real counts; None = fall back to token_count.
    let mut actual_prompt_tokens: Option<u32> = None;
    let mut actual_completion_tokens: Option<u32> = None;
    let mut actual_cached_tokens: Option<u32> = None;
    // Time to first token (ms from started_at to first non-final non-empty token).
    let mut ttft_ms_value: Option<i32> = None;

    loop {
        // biased: cancel branch is checked first so cancellation fires immediately
        // without waiting for the next token from Ollama.  Dropping token_stream
        // closes the TCP connection, which sends a broken-pipe to Ollama and stops
        // its generation loop.
        let result = tokio::select! {
            biased;
            _ = cancel_notify.notified() => {
                tracing::info!(%uuid, "job cancelled — dropping Ollama stream");
                return Ok(());
            }
            item = token_stream.next() => item,
        };

        let result = match result {
            Some(r) => r,
            None => break,
        };

        let mut entry = match jobs.get_mut(&uuid) {
            Some(e) => e,
            None => break,
        };

        if entry.status == JobStatus::Cancelled {
            return Ok(());
        }

        match result {
            Ok(token) => {
                token_count += 1;
                accumulated_text.push_str(&token.value);
                // Collect tool calls into a structured Vec for JSONB storage.
                // The SSE handler already forwards them to the client; here we
                // persist them separately so the dashboard and training exports
                // can query tool_name / arguments without parsing result_text.
                if let Some(ref tc) = token.tool_calls {
                    match tc {
                        serde_json::Value::Array(arr) => accumulated_tool_calls.extend(arr.iter().cloned()),
                        other => accumulated_tool_calls.push(other.clone()),
                    }
                }
                // Capture actual token counts from backend usage metadata.
                if token.prompt_tokens.is_some() || token.completion_tokens.is_some() {
                    actual_prompt_tokens = token.prompt_tokens;
                    actual_completion_tokens = token.completion_tokens;
                    actual_cached_tokens = token.cached_tokens;
                }
                // Record TTFT on the first non-empty, non-final token.
                if ttft_ms_value.is_none() && !token.is_final && !token.value.is_empty() {
                    ttft_ms_value = Some(
                        chrono::Utc::now()
                            .signed_duration_since(started_at)
                            .num_milliseconds()
                            .max(0) as i32,
                    );
                }
                // If the final token carries text, split it into a text token
                // followed by a separate done marker so the SSE handler never
                // discards text that arrives on the same chunk as is_final=true.
                if token.is_final && !token.value.is_empty() {
                    entry.tokens.push(StreamToken { value: token.value, is_final: false, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None });
                    entry.tokens.push(StreamToken { value: String::new(), is_final: true, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None });
                } else {
                    entry.tokens.push(token);
                }
                let notify = entry.notify.clone();
                drop(entry); // drop RefMut before notify_one (not strictly required, but safe)
                notify.notify_one();
            }
            Err(e) => {
                let error_msg = e.to_string();
                entry.status = JobStatus::Failed;
                entry.job.status = JobStatus::Failed;
                entry.job.error = Some(error_msg.clone());
                entry.done = true;
                let notify = entry.notify.clone();
                drop(entry); // drop RefMut before await
                notify.notify_one();

                job.status = JobStatus::Failed;
                job.error = Some(error_msg.clone());
                if let Err(db_err) = job_repo.save(&job).await {
                    tracing::warn!(job_id = %uuid, "failed to persist failed state: {db_err}");
                }

                // ── Record observability event (failed) ──────────────────────
                let completed_at = chrono::Utc::now();
                let latency_ms = completed_at
                    .signed_duration_since(started_at)
                    .num_milliseconds()
                    .max(0) as u32;

                emit_inference_event(
                    &observability,
                    uuid,
                    api_key_id,
                    &job,
                    actual_prompt_tokens.unwrap_or(0),
                    actual_completion_tokens.unwrap_or(token_count as u32),
                    latency_ms,
                    FinishReason::Error,
                    "failed".to_string(),
                    Some(error_msg),
                )
                .await;

                return Err(e);
            }
        }
    }

    // ── Completed ────────────────────────────────────────────────────
    let completed_at = chrono::Utc::now();
    let final_status = if let Some(mut entry) = jobs.get_mut(&uuid) {
        if entry.status != JobStatus::Cancelled {
            entry.status = JobStatus::Completed;
            entry.job.status = JobStatus::Completed;
            entry.job.completed_at = Some(completed_at);
            entry.done = true;
            let notify = entry.notify.clone();
            drop(entry); // drop RefMut before notify_one
            notify.notify_one();
            JobStatus::Completed
        } else {
            JobStatus::Cancelled
        }
    } else {
        JobStatus::Completed
    };

    let result_text = if accumulated_text.is_empty() {
        None
    } else {
        Some(accumulated_text)
    };

    let tool_calls_json = if accumulated_tool_calls.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(accumulated_tool_calls))
    };

    let stored_latency_ms = completed_at
        .signed_duration_since(started_at)
        .num_milliseconds()
        .max(0) as i32;

    let stored_completion_tokens = actual_completion_tokens
        .map(|v| v as i32)
        .or_else(|| if token_count > 0 { Some(token_count as i32) } else { None });

    // Mutate job fields directly for the final save — avoids cloning all 16+ fields.
    job.status = JobStatus::Completed;
    job.completed_at = Some(completed_at);
    job.result_text = result_text.clone();
    job.tool_calls_json = tool_calls_json;
    job.latency_ms = Some(stored_latency_ms);
    job.ttft_ms = ttft_ms_value;
    job.prompt_tokens = actual_prompt_tokens.map(|v| v as i32);
    job.completion_tokens = stored_completion_tokens;
    job.cached_tokens = actual_cached_tokens.map(|v| v as i32);
    if let Err(e) = job_repo.save(&job).await {
        tracing::warn!(job_id = %uuid, "failed to persist completed state: {e}");
    }

    let _ = event_tx.send(JobStatusEvent {
        id: uuid.to_string(),
        status: match final_status {
            JobStatus::Cancelled => "cancelled",
            JobStatus::Failed => "failed",
            _ => "completed",
        }.to_string(),
        model_name: job.model_name.as_str().to_string(),
        provider_type: match job.provider_type { ProviderType::Ollama => "ollama", ProviderType::Gemini => "gemini" }.to_string(),
        latency_ms: Some(stored_latency_ms),
    });

    // ── Model manager: record LRU usage (Ollama only) ────────────────
    if job.provider_type == ProviderType::Ollama {
        if let Some(ref mm) = model_manager {
            mm.record_used(job.model_name.as_str()).await;
        }
    }

    // ── Record TPM ───────────────────────────────────────────────────
    if let (Some(pool), Some(key_id)) = (&valkey_pool, api_key_id) {
        if let Err(e) = record_tpm(pool, key_id, token_count).await {
            tracing::warn!(job_id = %uuid, "failed to record TPM usage: {e}");
        }
    }

    // ── Increment Gemini RPM/RPD counters (free-tier only) ────────
    // Counters are only tracked for free-tier providers: paid providers
    // have no RPM/RPD limits to enforce, so counting is unnecessary.
    if job.provider_type == ProviderType::Gemini && provider_is_free_tier {
        if let Some(pid) = provider_id {
            if let Err(e) = provider_dispatch.increment_gemini_counters(pid, job.model_name.as_str()).await {
                tracing::warn!(job_id = %uuid, "failed to increment Gemini rate limit counters: {e}");
            }
        }
    }

    // ── Record observability event (completed / cancelled) ───────────
    let latency_ms = completed_at
        .signed_duration_since(started_at)
        .num_milliseconds()
        .max(0) as u32;

    let (finish_reason, status_str) = match final_status {
        JobStatus::Cancelled => (FinishReason::Cancelled, "cancelled".to_string()),
        _ => (FinishReason::Stop, "completed".to_string()),
    };

    emit_inference_event(
        &observability,
        uuid,
        api_key_id,
        &job,
        actual_prompt_tokens.unwrap_or(0),
        actual_completion_tokens.unwrap_or(token_count as u32),
        latency_ms,
        finish_reason,
        status_str,
        None,
    )
    .await;

    Ok(())
}

// ── Observability helper ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn emit_inference_event(
    observability: &Option<Arc<dyn ObservabilityPort>>,
    uuid: Uuid,
    api_key_id: Option<Uuid>,
    job: &InferenceJob,
    prompt_tokens: u32,
    completion_tokens: u32,
    latency_ms: u32,
    finish_reason: FinishReason,
    status: String,
    error_msg: Option<String>,
) {
    if let Some(obs) = observability {
        let backend_str = match job.provider_type {
            ProviderType::Ollama => "ollama".to_string(),
            ProviderType::Gemini => "gemini".to_string(),
        };

        let event = InferenceEvent {
            event_time: chrono::Utc::now(),
            request_id: uuid,
            api_key_id,
            tenant_id: String::new(),
            model_name: job.model_name.as_str().to_string(),
            provider_type: backend_str,
            prompt_tokens,
            completion_tokens,
            latency_ms,
            ttft_ms: None,
            finish_reason,
            status,
            error_msg,
        };

        if let Err(e) = obs.record_inference(&event).await {
            tracing::warn!(job_id = %uuid, "observability record failed (non-fatal): {e}");
        }
    }
}

// ── TPM accounting ─────────────────────────────────────────────────────────────

/// Increment the per-minute token counter for an API key.
///
/// Key pattern: `veronex:ratelimit:tpm:{key_id}:{minute}`
/// TTL is set to 2 minutes so stale keys are cleaned up automatically.
pub async fn record_tpm(
    pool: &fred::clients::Pool,
    api_key_id: Uuid,
    tokens: u64,
) -> anyhow::Result<()> {
    use fred::prelude::*;

    if tokens == 0 {
        return Ok(());
    }

    let minute = chrono::Utc::now().timestamp() / 60;
    let key = format!("veronex:ratelimit:tpm:{}:{}", api_key_id, minute);

    let _: i64 = pool.incr_by(&key, tokens as i64).await?;
    let _: bool = pool.expire(&key, 120, None).await?;

    Ok(())
}
