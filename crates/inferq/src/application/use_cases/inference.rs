use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::inference_backend::InferenceBackendPort;
use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::application::ports::outbound::model_manager_port::ModelManagerPort;
use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{BackendType, FinishReason, JobStatus};
use crate::domain::value_objects::{JobId, ModelName, Prompt, StreamToken};
use crate::infrastructure::outbound::backend_router::{get_ollama_available_vram_mb, make_adapter, pick_best_backend};

// ── Queue key ──────────────────────────────────────────────────────────────────

const QUEUE_KEY: &str = "inferq:queue:jobs";

// ── In-memory job store ────────────────────────────────────────────────────────

struct JobEntry {
    job: InferenceJob,
    status: JobStatus,
    tokens: Vec<StreamToken>,
    done: bool,
    /// API key that submitted this job — used for TPM accounting.
    api_key_id: Option<Uuid>,
    notify: Arc<Notify>,
}

// ── Use-case implementation ────────────────────────────────────────────────────

pub struct InferenceUseCaseImpl {
    /// Registry of all registered backends (Ollama servers, Gemini keys).
    /// Used at dispatch time to pick the best available backend via VRAM check.
    registry: Arc<dyn LlmBackendRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: Option<fred::clients::RedisPool>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    jobs: Arc<Mutex<HashMap<Uuid, JobEntry>>>,
    /// Tracks which backend IDs are currently processing a job.
    /// Prevents double-dispatching to the same backend before VRAM state updates.
    busy_backends: Arc<std::sync::Mutex<HashSet<Uuid>>>,
}

impl InferenceUseCaseImpl {
    pub fn new(
        registry: Arc<dyn LlmBackendRegistry>,
        job_repo: Arc<dyn JobRepository>,
        valkey_pool: Option<fred::clients::RedisPool>,
        observability: Option<Arc<dyn ObservabilityPort>>,
        model_manager: Option<Arc<dyn ModelManagerPort>>,
    ) -> Self {
        Self {
            registry,
            job_repo,
            valkey_pool,
            observability,
            model_manager,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            busy_backends: Arc::new(std::sync::Mutex::new(HashSet::new())),
        }
    }

    /// Spawn the multi-backend queue dispatcher (no-op if Valkey is not configured).
    ///
    /// The dispatcher pops jobs from the Valkey queue, finds the backend with the most
    /// available VRAM (via Ollama's `/api/ps`), and spawns each job concurrently.
    /// Each physical GPU (Ollama server) processes one job at a time; multiple GPUs
    /// run in parallel. If no backend has capacity, the job is re-queued and the
    /// dispatcher backs off briefly.
    pub fn start_queue_worker(&self) {
        let Some(ref pool) = self.valkey_pool else {
            return;
        };

        let jobs = self.jobs.clone();
        let registry = self.registry.clone();
        let job_repo = self.job_repo.clone();
        let valkey_pool = pool.clone();
        let observability = self.observability.clone();
        let model_manager = self.model_manager.clone();
        let busy_backends = self.busy_backends.clone();

        tokio::spawn(async move {
            queue_dispatcher_loop(
                jobs,
                registry,
                job_repo,
                valkey_pool,
                observability,
                model_manager,
                busy_backends,
            )
            .await;
        });

        tracing::info!("multi-backend queue dispatcher started (VRAM-aware routing)");
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

            {
                let mut guard = self.jobs.lock().await;
                guard.entry(uuid).or_insert_with(|| JobEntry {
                    job: job.clone(),
                    status: job.status,
                    tokens: Vec::new(),
                    done: false,
                    api_key_id: None,
                    notify: Arc::new(Notify::new()),
                });
            }

            if let Err(e) = pool.rpush::<i64, _, _>(QUEUE_KEY, uuid.to_string()).await {
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
    ) -> Result<JobId> {
        let job_id = JobId::new();
        let backend = match backend_type {
            "gemini" => BackendType::Gemini,
            _ => BackendType::Ollama,
        };

        let job = InferenceJob {
            id: job_id.clone(),
            prompt: Prompt::new(prompt)?,
            model_name: ModelName::new(model_name)?,
            status: JobStatus::Pending,
            backend,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
        };

        self.job_repo.save(&job).await?;

        {
            let mut guard = self.jobs.lock().await;
            guard.insert(
                job_id.0,
                JobEntry {
                    job: job.clone(),
                    status: JobStatus::Pending,
                    tokens: Vec::new(),
                    done: false,
                    api_key_id,
                    notify: Arc::new(Notify::new()),
                },
            );
        }

        let uuid = job_id.0;

        if let Some(ref pool) = self.valkey_pool {
            // Persistent queue: RPUSH job UUID — dispatcher picks it up.
            use fred::prelude::*;
            match pool.rpush::<i64, _, _>(QUEUE_KEY, uuid.to_string()).await {
                Ok(_) => {
                    tracing::debug!(%uuid, "job enqueued to Valkey queue");
                }
                Err(e) => {
                    tracing::warn!(%uuid, "Valkey enqueue failed, falling back to direct spawn: {e}");
                    spawn_job_direct(
                        self.jobs.clone(),
                        self.registry.clone(),
                        self.job_repo.clone(),
                        self.valkey_pool.clone(),
                        self.observability.clone(),
                        self.model_manager.clone(),
                        self.busy_backends.clone(),
                        uuid,
                        job,
                    );
                }
            }
        } else {
            // No Valkey — immediate spawn (dev mode, direct registry dispatch).
            spawn_job_direct(
                self.jobs.clone(),
                self.registry.clone(),
                self.job_repo.clone(),
                None,
                self.observability.clone(),
                self.model_manager.clone(),
                self.busy_backends.clone(),
                uuid,
                job,
            );
        }

        Ok(job_id)
    }

    async fn process(&self, job_id: &JobId) -> Result<()> {
        let uuid = job_id.0;
        let (job, api_key_id) = {
            let guard = self.jobs.lock().await;
            let entry = guard
                .get(&uuid)
                .ok_or_else(|| anyhow::anyhow!("job not found: {uuid}"))?;

            if matches!(
                entry.status,
                JobStatus::Running | JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
            ) {
                return Ok(());
            }

            (entry.job.clone(), entry.api_key_id)
        };
        let _ = api_key_id; // used in spawned path; process() ignores it

        // For process(), pick the best available backend now.
        let backend = match pick_best_backend(&*self.registry, &job.backend).await {
            Ok(cfg) => make_adapter(&cfg),
            Err(e) => return Err(e),
        };

        run_job(
            self.jobs.clone(),
            backend,
            self.job_repo.clone(),
            self.valkey_pool.clone(),
            self.observability.clone(),
            self.model_manager.clone(),
            uuid,
            job,
        )
        .await
    }

    fn stream(&self, job_id: &JobId) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let jobs = self.jobs.clone();
        let uuid = job_id.0;

        Box::pin(async_stream::try_stream! {
            let mut idx: usize = 0;

            loop {
                let (new_tokens, done, notify) = {
                    let guard = jobs.lock().await;
                    let entry = guard
                        .get(&uuid)
                        .ok_or_else(|| anyhow::anyhow!("job not found: {uuid}"))?;

                    let new_tokens = entry.tokens[idx..].to_vec();
                    let done = entry.done;
                    let notify = entry.notify.clone();
                    (new_tokens, done, notify)
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
        {
            let guard = self.jobs.lock().await;
            if let Some(entry) = guard.get(&job_id.0) {
                return Ok(entry.status);
            }
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
        {
            let mut guard = self.jobs.lock().await;
            if let Some(entry) = guard.get_mut(&job_id.0) {
                entry.status = JobStatus::Cancelled;
                entry.done = true;
                entry.notify.notify_one();
            }
        }
        self.job_repo
            .update_status(job_id, JobStatus::Cancelled)
            .await?;
        Ok(())
    }
}

// ── Direct spawn helper (no-Valkey dev mode) ──────────────────────────────────

fn spawn_job_direct(
    jobs: Arc<Mutex<HashMap<Uuid, JobEntry>>>,
    registry: Arc<dyn LlmBackendRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: Option<fred::clients::RedisPool>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    busy_backends: Arc<std::sync::Mutex<HashSet<Uuid>>>,
    uuid: Uuid,
    job: InferenceJob,
) {
    tokio::spawn(async move {
        // Pick a backend from the registry.
        let backend_cfg = match pick_best_backend(&*registry, &job.backend).await {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!(job_id = %uuid, "no backend available: {e}");
                return;
            }
        };
        let backend_id = backend_cfg.id;
        busy_backends.lock().unwrap().insert(backend_id);
        let adapter = make_adapter(&backend_cfg);

        if let Err(e) = run_job(
            jobs,
            adapter,
            job_repo,
            valkey_pool,
            observability,
            model_manager,
            uuid,
            job,
        )
        .await
        {
            tracing::error!(job_id = %uuid, "inference job failed: {e}");
        }
        busy_backends.lock().unwrap().remove(&backend_id);
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
    jobs: Arc<Mutex<HashMap<Uuid, JobEntry>>>,
    registry: Arc<dyn LlmBackendRegistry>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: fred::clients::RedisPool,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    busy_backends: Arc<std::sync::Mutex<HashSet<Uuid>>>,
) {
    use fred::prelude::*;

    tracing::info!("queue dispatcher loop started, waiting for jobs on {QUEUE_KEY}");

    loop {
        // BLPOP blocks for up to 5 s; returns None on timeout.
        let result: Result<Option<(String, String)>, _> =
            valkey_pool.blpop(QUEUE_KEY, 5.0).await;

        let payload = match result {
            Ok(None) => continue,
            Ok(Some((_key, value))) => value,
            Err(e) if matches!(e.kind(), fred::error::RedisErrorKind::Timeout) => continue,
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
        let job = {
            let guard = jobs.lock().await;
            if let Some(entry) = guard.get(&uuid) {
                entry.job.clone()
            } else {
                drop(guard);
                let job_id = crate::domain::value_objects::JobId(uuid);
                match job_repo.get(&job_id).await {
                    Ok(Some(j)) => {
                        let mut guard = jobs.lock().await;
                        guard.entry(uuid).or_insert_with(|| JobEntry {
                            job: j.clone(),
                            status: j.status,
                            tokens: Vec::new(),
                            done: false,
                            api_key_id: None,
                            notify: Arc::new(Notify::new()),
                        });
                        j
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
            }
        };

        // ── Find an available backend (VRAM check) and claim it atomically ──
        let backend_cfg = registry.list_all().await.unwrap_or_default();
        let candidates: Vec<_> = backend_cfg
            .into_iter()
            .filter(|b| b.is_active && b.backend_type == job.backend)
            .collect();

        // Collect VRAM availability for each candidate.
        let mut availability: Vec<(crate::domain::entities::LlmBackend, i64)> = Vec::new();
        for b in candidates {
            let avail = match b.backend_type {
                BackendType::Ollama => get_ollama_available_vram_mb(&b).await,
                BackendType::Gemini => i64::MAX, // no VRAM constraint
            };
            availability.push((b, avail));
        }
        // Sort by most available VRAM descending.
        availability.sort_by(|a, b| b.1.cmp(&a.1));

        // Atomically claim the best non-busy backend.
        let claimed = {
            let mut busy = busy_backends.lock().unwrap();
            availability
                .into_iter()
                .find(|(b, avail)| !busy.contains(&b.id) && *avail > 0)
                .map(|(b, _)| {
                    busy.insert(b.id);
                    b
                })
        };

        match claimed {
            Some(backend_cfg) => {
                let backend_id = backend_cfg.id;
                let adapter = make_adapter(&backend_cfg);

                tracing::info!(
                    %uuid,
                    backend_id = %backend_id,
                    backend_name = %backend_cfg.name,
                    "dispatching job to backend"
                );

                let jobs_c = jobs.clone();
                let job_repo_c = job_repo.clone();
                let valkey_c = valkey_pool.clone();
                let obs_c = observability.clone();
                let mm_c = model_manager.clone();
                let busy_c = busy_backends.clone();

                tokio::spawn(async move {
                    if let Err(e) = run_job(
                        jobs_c,
                        adapter,
                        job_repo_c,
                        Some(valkey_c),
                        obs_c,
                        mm_c,
                        uuid,
                        job,
                    )
                    .await
                    {
                        tracing::error!(%uuid, %backend_id, "inference job failed: {e}");
                    }
                    busy_c.lock().unwrap().remove(&backend_id);
                    tracing::debug!(%backend_id, "backend released");
                });
            }
            None => {
                // No backend available → put job back at front of queue and wait.
                tracing::debug!(%uuid, "no available backend, re-queuing");
                if let Err(e) = valkey_pool
                    .lpush::<i64, _, _>(QUEUE_KEY, uuid.to_string())
                    .await
                {
                    tracing::error!(%uuid, "failed to re-queue job: {e}");
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}

// ── Background job runner ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_job(
    jobs: Arc<Mutex<HashMap<Uuid, JobEntry>>>,
    backend: Arc<dyn InferenceBackendPort>,
    job_repo: Arc<dyn JobRepository>,
    valkey_pool: Option<fred::clients::RedisPool>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    uuid: Uuid,
    job: InferenceJob,
) -> Result<()> {
    // ── Model manager: ensure model is loaded (Ollama only) ──────────
    if job.backend == BackendType::Ollama {
        if let Some(ref mm) = model_manager {
            if let Err(e) = mm.ensure_loaded(job.model_name.as_str()).await {
                tracing::warn!(%uuid, "model manager ensure_loaded failed (non-fatal): {e}");
            }
        }
    }

    // ── Running ──────────────────────────────────────────────────────
    let started_at = chrono::Utc::now();
    let api_key_id = {
        let mut guard = jobs.lock().await;
        if let Some(entry) = guard.get_mut(&uuid) {
            if entry.status == JobStatus::Cancelled {
                return Ok(());
            }
            entry.status = JobStatus::Running;
            entry.job.status = JobStatus::Running;
            entry.job.started_at = Some(started_at);
            entry.api_key_id
        } else {
            None
        }
    };

    if let Err(e) = job_repo
        .save(&InferenceJob {
            status: JobStatus::Running,
            started_at: Some(started_at),
            ..job.clone()
        })
        .await
    {
        tracing::warn!(job_id = %uuid, "failed to persist running state: {e}");
    }

    // ── Stream tokens ────────────────────────────────────────────────
    let mut token_stream = backend.stream_tokens(&job);
    let mut token_count: u64 = 0;

    while let Some(result) = token_stream.next().await {
        let mut guard = jobs.lock().await;
        let entry = match guard.get_mut(&uuid) {
            Some(e) => e,
            None => break,
        };

        if entry.status == JobStatus::Cancelled {
            return Ok(());
        }

        match result {
            Ok(token) => {
                token_count += 1;
                entry.tokens.push(token);
                entry.notify.notify_one();
            }
            Err(e) => {
                let error_msg = e.to_string();
                entry.status = JobStatus::Failed;
                entry.job.status = JobStatus::Failed;
                entry.job.error = Some(error_msg.clone());
                entry.done = true;
                entry.notify.notify_one();
                drop(guard);

                if let Err(db_err) = job_repo
                    .save(&InferenceJob {
                        status: JobStatus::Failed,
                        error: Some(error_msg.clone()),
                        started_at: Some(started_at),
                        ..job.clone()
                    })
                    .await
                {
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
                    token_count as u32,
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
    let final_status = {
        let mut guard = jobs.lock().await;
        if let Some(entry) = guard.get_mut(&uuid) {
            if entry.status != JobStatus::Cancelled {
                entry.status = JobStatus::Completed;
                entry.job.status = JobStatus::Completed;
                entry.job.completed_at = Some(completed_at);
                entry.done = true;
                entry.notify.notify_one();
                JobStatus::Completed
            } else {
                JobStatus::Cancelled
            }
        } else {
            JobStatus::Completed
        }
    };

    if let Err(e) = job_repo
        .save(&InferenceJob {
            status: JobStatus::Completed,
            started_at: Some(started_at),
            completed_at: Some(completed_at),
            ..job.clone()
        })
        .await
    {
        tracing::warn!(job_id = %uuid, "failed to persist completed state: {e}");
    }

    // ── Model manager: record LRU usage (Ollama only) ────────────────
    if job.backend == BackendType::Ollama {
        if let Some(ref mm) = model_manager {
            mm.record_used(job.model_name.as_str()).await;
        }
    }

    // ── Record TPM ───────────────────────────────────────────────────
    if let (Some(pool), Some(key_id)) = (valkey_pool, api_key_id) {
        if let Err(e) = record_tpm(&pool, key_id, token_count).await {
            tracing::warn!(job_id = %uuid, "failed to record TPM usage: {e}");
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
        token_count as u32,
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
    completion_tokens: u32,
    latency_ms: u32,
    finish_reason: FinishReason,
    status: String,
    error_msg: Option<String>,
) {
    if let Some(obs) = observability {
        let backend_str = match job.backend {
            BackendType::Ollama => "ollama".to_string(),
            BackendType::Gemini => "gemini".to_string(),
        };

        let event = InferenceEvent {
            event_time: chrono::Utc::now(),
            request_id: uuid,
            api_key_id,
            tenant_id: String::new(),
            model_name: job.model_name.as_str().to_string(),
            backend: backend_str,
            prompt_tokens: 0,
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
/// Key pattern: `inferq:ratelimit:tpm:{key_id}:{minute}`
/// TTL is set to 2 minutes so stale keys are cleaned up automatically.
pub async fn record_tpm(
    pool: &fred::clients::RedisPool,
    api_key_id: Uuid,
    tokens: u64,
) -> anyhow::Result<()> {
    use fred::prelude::*;

    if tokens == 0 {
        return Ok(());
    }

    let minute = chrono::Utc::now().timestamp() / 60;
    let key = format!("inferq:ratelimit:tpm:{}:{}", api_key_id, minute);

    let _: i64 = pool.incr_by(&key, tokens as i64).await?;
    let _: bool = pool.expire(&key, 120).await?;

    Ok(())
}
