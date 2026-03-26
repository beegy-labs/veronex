use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;
use futures::StreamExt as _;
use tokio::sync::{broadcast, Notify};
use uuid::Uuid;

use crate::application::ports::outbound::inference_provider::InferenceProviderPort;
use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::model_manager_port::ModelManagerPort;
use crate::application::ports::outbound::observability_port::ObservabilityPort;
use crate::application::ports::outbound::provider_dispatch_port::ProviderDispatchPort;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::{FinishReason, JobStatus, ProviderType};
use crate::domain::value_objects::{JobStatusEvent, StreamToken};
use crate::domain::constants::{
    JOB_CLEANUP_DELAY, JOB_OWNER_TTL_SECS, MAX_TOKENS_PER_JOB,
    OWNER_REFRESH_INTERVAL, OWNERSHIP_LOST_CLEANUP_DELAY,
};

use super::JobEntry;
use super::helpers::{broadcast_event, decr_pending, decr_running, emit_inference_event, incr_running, record_tpm, schedule_cleanup};

// ── Token stream state ──────────────────────────────────────────────────────

struct TokenStreamState {
    token_count: u64,
    text: String,
    last_owner_refresh: std::time::Instant,
    tool_calls: Vec<serde_json::Value>,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    cached_tokens: Option<u32>,
    ttft_ms: Option<i32>,
}

impl Default for TokenStreamState {
    fn default() -> Self {
        Self {
            token_count: 0,
            text: String::new(),
            last_owner_refresh: std::time::Instant::now(),
            tool_calls: Vec::new(),
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            ttft_ms: None,
        }
    }
}

// ── Stream error handler ────────────────────────────────────────────────────

/// Handle a provider stream error: persist failure, emit observability, refund TPM.
#[allow(clippy::too_many_arguments)]
async fn handle_stream_error(
    jobs: &Arc<DashMap<Uuid, JobEntry>>,
    job: &mut InferenceJob,
    job_repo: &dyn JobRepository,
    observability: &Option<Arc<dyn ObservabilityPort>>,
    valkey: &Option<Arc<dyn ValkeyPort>>,
    uuid: Uuid,
    started_at: chrono::DateTime<chrono::Utc>,
    api_key_id: Option<Uuid>,
    tpm_minute: Option<i64>,
    ts: &TokenStreamState,
    error: &anyhow::Error,
) {
    let msg = error.to_string();

    if let Some(mut entry) = jobs.get_mut(&uuid) {
        entry.status = JobStatus::Failed;
        entry.job.status = JobStatus::Failed;
        entry.job.error = Some(msg.clone());
        entry.job.failure_reason = Some("provider_error".to_string());
        entry.done = true;
        let notify = entry.notify.clone();
        drop(entry);
        notify.notify_one();
    }

    job.status = JobStatus::Failed;
    job.error = Some(msg.clone());
    job.failure_reason = Some("provider_error".to_string());
    if let Err(db_err) = job_repo
        .fail_with_reason(&job.id, "provider_error", Some(&msg))
        .await
    {
        tracing::warn!(job_id = %uuid, "failed to persist failed state: {db_err}");
    }

    // running → failed: DECR running
    decr_running(valkey).await;

    let latency_ms = chrono::Utc::now()
        .signed_duration_since(started_at).num_milliseconds().max(0) as u32;
    emit_inference_event(
        observability, uuid, api_key_id, job,
        ts.prompt_tokens.unwrap_or(0),
        ts.completion_tokens.unwrap_or(ts.token_count as u32),
        latency_ms, FinishReason::Error, "failed".into(), Some(msg),
    ).await;

    // Refund TPM reservation
    if let (Some(vk), Some(key_id)) = (valkey, api_key_id)
        && let Err(e) = record_tpm(vk.as_ref(), key_id, 0, tpm_minute).await
    {
        tracing::warn!(job_id = %uuid, "failed to refund TPM: {e}");
    }

    schedule_cleanup(jobs, uuid, JOB_CLEANUP_DELAY);
}

// ── Job finalizer ───────────────────────────────────────────────────────────

/// Finalize a completed job: ownership guard, persist results, broadcast, record metrics.
///
/// Returns `Some(latency_ms)` if the job completed normally, `None` if cancelled
/// or ownership was lost to another node.
#[allow(clippy::too_many_arguments)]
async fn finalize_job(
    jobs: &Arc<DashMap<Uuid, JobEntry>>,
    job: &mut InferenceJob,
    job_repo: &dyn JobRepository,
    valkey: &Option<Arc<dyn ValkeyPort>>,
    observability: &Option<Arc<dyn ObservabilityPort>>,
    model_manager: &Option<Arc<dyn ModelManagerPort>>,
    provider_dispatch: &dyn ProviderDispatchPort,
    event_tx: &broadcast::Sender<JobStatusEvent>,
    instance_id: &Arc<str>,
    cancel_notifiers: &DashMap<Uuid, Arc<Notify>>,
    uuid: Uuid,
    started_at: chrono::DateTime<chrono::Utc>,
    ts: TokenStreamState,
    api_key_id: Option<Uuid>,
    tpm_minute: Option<i64>,
    provider_id: Option<Uuid>,
    provider_is_free_tier: bool,
) -> Option<u32> {
    let completed_at = chrono::Utc::now();

    // Mark in-memory entry as completed
    let final_status = if let Some(mut entry) = jobs.get_mut(&uuid) {
        if entry.status != JobStatus::Cancelled {
            entry.status = JobStatus::Completed;
            entry.job.status = JobStatus::Completed;
            entry.job.completed_at = Some(completed_at);
            entry.done = true;
            let notify = entry.notify.clone();
            drop(entry);
            notify.notify_one();
            JobStatus::Completed
        } else {
            JobStatus::Cancelled
        }
    } else {
        JobStatus::Completed
    };

    // running → completed/cancelled: DECR running
    decr_running(valkey).await;

    let result_text = (!ts.text.is_empty()).then_some(ts.text);
    let tool_calls_json = (!ts.tool_calls.is_empty())
        .then_some(serde_json::Value::Array(ts.tool_calls));

    let latency_ms_raw = completed_at.signed_duration_since(started_at).num_milliseconds().max(0);
    let stored_latency = latency_ms_raw as i32;
    let stored_completion = ts.completion_tokens.map(|v| v as i32)
        .or_else(|| (ts.token_count > 0).then_some(ts.token_count as i32));

    // Ownership guard: prevent double-write if reaper re-enqueued
    if let Some(vk) = valkey {
        let owner_key = crate::domain::constants::job_owner_key(uuid);
        if let Ok(Some(id)) = vk.kv_get(&owner_key).await
            && id != instance_id.as_ref()
        {
            tracing::warn!(%uuid, current_owner = %id, "ownership lost — aborting");
            if let Some(mut entry) = jobs.get_mut(&uuid) { entry.done = true; }
            cancel_notifiers.remove(&uuid);
            schedule_cleanup(jobs, uuid, OWNERSHIP_LOST_CLEANUP_DELAY);
            return None;
        }
    }

    // Persist completed state
    job.status = JobStatus::Completed;
    job.completed_at = Some(completed_at);
    job.result_text = result_text;
    job.tool_calls_json = tool_calls_json;
    job.latency_ms = Some(stored_latency);
    job.ttft_ms = ts.ttft_ms;
    job.prompt_tokens = ts.prompt_tokens.map(|v| v as i32);
    job.completion_tokens = stored_completion;
    job.cached_tokens = ts.cached_tokens.map(|v| v as i32);
    if let Err(e) = job_repo
        .mark_completed(
            &job.id,
            completed_at,
            job.result_text.as_deref(),
            job.tool_calls_json.as_ref(),
            stored_latency,
            job.ttft_ms,
            job.prompt_tokens,
            job.completion_tokens,
            job.cached_tokens,
        )
        .await
    {
        tracing::warn!(job_id = %uuid, "failed to persist completed state: {e}");
    }

    // Broadcast status event
    broadcast_event(event_tx, valkey, instance_id, &JobStatusEvent {
        id: uuid.to_string(),
        status: match final_status {
            JobStatus::Cancelled => "cancelled",
            JobStatus::Failed => "failed",
            _ => "completed",
        }.into(),
        model_name: job.model_name.as_str().into(),
        provider_type: job.provider_type.as_str().into(),
        latency_ms: Some(stored_latency),
    }).await;

    cancel_notifiers.remove(&uuid);

    // Record LRU usage (Ollama only)
    if job.provider_type == ProviderType::Ollama
        && let Some(mm) = model_manager
    {
        mm.record_used(job.model_name.as_str()).await;
    }

    // Record TPM
    if let (Some(vk), Some(key_id)) = (valkey, api_key_id)
        && let Err(e) = record_tpm(vk.as_ref(), key_id, ts.token_count, tpm_minute).await
    {
        tracing::warn!(job_id = %uuid, "failed to record TPM: {e}");
    }

    // Gemini RPM/RPD counters (free-tier only)
    if job.provider_type == ProviderType::Gemini && provider_is_free_tier
        && let Some(pid) = provider_id
        && let Err(e) = provider_dispatch.increment_gemini_counters(pid, job.model_name.as_str()).await
    {
        tracing::warn!(job_id = %uuid, "failed to increment Gemini counters: {e}");
    }

    // Observability event
    let (reason, status) = match final_status {
        JobStatus::Cancelled => (FinishReason::Cancelled, "cancelled".into()),
        _ => (FinishReason::Stop, "completed".into()),
    };
    emit_inference_event(
        observability, uuid, api_key_id, job,
        ts.prompt_tokens.unwrap_or(0),
        ts.completion_tokens.unwrap_or(ts.token_count as u32),
        latency_ms_raw as u32, reason, status, None,
    ).await;

    schedule_cleanup(jobs, uuid, JOB_CLEANUP_DELAY);
    Some(latency_ms_raw as u32)
}

// ── Job runner ──────────────────────────────────────────────────────────────

/// Run a single inference job: setup → stream tokens → finalize.
///
/// Returns `Ok(Some(latency_ms))` on successful completion,
/// `Ok(None)` if the job was cancelled or ownership was lost,
/// `Err` on provider stream failure.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_job(
    jobs: Arc<DashMap<Uuid, JobEntry>>,
    provider: Arc<dyn InferenceProviderPort>,
    job_repo: Arc<dyn JobRepository>,
    valkey: Option<Arc<dyn ValkeyPort>>,
    observability: Option<Arc<dyn ObservabilityPort>>,
    model_manager: Option<Arc<dyn ModelManagerPort>>,
    provider_dispatch: Arc<dyn ProviderDispatchPort>,
    uuid: Uuid,
    mut job: InferenceJob,
    provider_id: Option<Uuid>,
    provider_is_free_tier: bool,
    event_tx: broadcast::Sender<JobStatusEvent>,
    instance_id: Arc<str>,
    cancel_notifiers: Arc<DashMap<Uuid, Arc<Notify>>>,
) -> Result<Option<u32>> {
    // ── Setup ──────────────────────────────────────────────────────────
    if job.provider_type == ProviderType::Ollama
        && let Some(ref mm) = model_manager
        && let Err(e) = mm.ensure_loaded(job.model_name.as_str()).await
    {
        tracing::warn!(%uuid, "model manager ensure_loaded failed (non-fatal): {e}");
    }

    let started_at = chrono::Utc::now();
    let (api_key_id, tpm_minute) = if let Some(mut entry) = jobs.get_mut(&uuid) {
        if entry.status == JobStatus::Cancelled {
            drop(entry);
            // pending → cancelled (before dispatch): DECR pending
            decr_pending(&valkey).await;
            return Ok(None);
        }
        entry.status = JobStatus::Running;
        entry.job.status = JobStatus::Running;
        entry.job.started_at = Some(started_at);
        (entry.api_key_id, entry.tpm_reservation_minute)
    } else {
        (None, None)
    };

    job.status = JobStatus::Running;
    job.started_at = Some(started_at);
    job.provider_id = provider_id;
    job.queue_time_ms = Some(
        started_at.signed_duration_since(job.created_at).num_milliseconds().max(0) as i32,
    );
    let q_ms = job.queue_time_ms.unwrap_or(0);
    if let Err(e) = job_repo.mark_running(&job.id, started_at, provider_id, q_ms).await {
        tracing::warn!(job_id = %uuid, "failed to persist running state: {e}");
    }

    // pending → running: DECR pending, INCR running
    decr_pending(&valkey).await;
    incr_running(&valkey).await;

    broadcast_event(&event_tx, &valkey, &instance_id, &JobStatusEvent {
        id: uuid.to_string(),
        status: "running".into(),
        model_name: job.model_name.as_str().into(),
        provider_type: job.provider_type.as_str().into(),
        latency_ms: None,
    }).await;

    // ── Stream tokens ──────────────────────────────────────────────────
    let cancel_notify = jobs.get(&uuid)
        .map(|e| e.cancel_notify.clone())
        .unwrap_or_else(|| Arc::new(Notify::new()));

    let mut stream = provider.stream_tokens(&job);
    job.messages = None; // S3 is authoritative
    let mut ts = TokenStreamState::default();

    loop {
        let result = tokio::select! {
            biased;
            _ = cancel_notify.notified() => {
                tracing::info!(%uuid, "job cancelled — dropping stream");
                // running → cancelled: DECR running
                decr_running(&valkey).await;
                schedule_cleanup(&jobs, uuid, JOB_CLEANUP_DELAY);
                return Ok(None);
            }
            item = stream.next() => item,
        };

        let Some(result) = result else { break };

        let mut entry = match jobs.get_mut(&uuid) {
            Some(e) => e,
            None => break,
        };

        if entry.status == JobStatus::Cancelled {
            drop(entry);
            // running → cancelled: DECR running
            decr_running(&valkey).await;
            return Ok(None);
        }

        match result {
            Ok(token) => {
                ts.token_count += 1;
                ts.text.push_str(&token.value);

                if let Some(ref tc) = token.tool_calls {
                    match tc {
                        serde_json::Value::Array(arr) => {
                            ts.tool_calls.reserve(arr.len());
                            ts.tool_calls.extend(arr.iter().cloned());
                        }
                        other => ts.tool_calls.push(other.clone()),
                    }
                }
                if token.prompt_tokens.is_some() || token.completion_tokens.is_some() {
                    ts.prompt_tokens = token.prompt_tokens;
                    ts.completion_tokens = token.completion_tokens;
                    ts.cached_tokens = token.cached_tokens;
                }
                if ts.ttft_ms.is_none() && !token.is_final && !token.value.is_empty() {
                    ts.ttft_ms = Some(
                        chrono::Utc::now().signed_duration_since(started_at)
                            .num_milliseconds().max(0) as i32,
                    );
                }

                // Token budget guard
                if entry.tokens.len() > MAX_TOKENS_PER_JOB {
                    entry.done = true;
                    entry.status = JobStatus::Failed;
                    entry.job.status = JobStatus::Failed;
                    entry.job.error = Some("token budget exceeded".into());
                    entry.job.failure_reason = Some("token_budget_exceeded".to_string());
                    let notify = entry.notify.clone();
                    drop(entry);
                    notify.notify_one();
                    tracing::warn!(job_id = %uuid, "token budget exceeded");
                    break;
                }

                // Split final token with text into text + done marker
                if token.is_final && !token.value.is_empty() {
                    entry.tokens.push(StreamToken::text(token.value));
                    entry.tokens.push(StreamToken::done());
                } else {
                    entry.tokens.push(token);
                }
                let notify = entry.notify.clone();
                drop(entry);
                notify.notify_one();

                // Periodic owner TTL refresh
                if ts.last_owner_refresh.elapsed() >= OWNER_REFRESH_INTERVAL {
                    if let Some(ref vk) = valkey {
                        let key = crate::domain::constants::job_owner_key(uuid);
                        let _ = vk.kv_set(&key, instance_id.as_ref(), JOB_OWNER_TTL_SECS, true).await;
                    }
                    ts.last_owner_refresh = std::time::Instant::now();
                }
            }
            Err(e) => {
                drop(entry);
                handle_stream_error(
                    &jobs, &mut job, job_repo.as_ref(), &observability, &valkey,
                    uuid, started_at, api_key_id, tpm_minute, &ts, &e,
                ).await;
                return Err(e);
            }
        }
    }

    // ── Finalize ───────────────────────────────────────────────────────
    let latency_ms = finalize_job(
        &jobs, &mut job, job_repo.as_ref(), &valkey, &observability,
        &model_manager, provider_dispatch.as_ref(), &event_tx, &instance_id,
        &cancel_notifiers, uuid, started_at, ts, api_key_id, tpm_minute,
        provider_id, provider_is_free_tier,
    ).await;

    Ok(latency_ms)
}
