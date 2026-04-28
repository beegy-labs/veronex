use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::Instrument;

use anyhow::Result;
use dashmap::DashMap;
use futures::StreamExt as _;
use tokio::sync::{broadcast, Notify};
use uuid::Uuid;

use crate::application::ports::outbound::inference_provider::LlmProviderPort;
use crate::application::ports::outbound::job_repository::JobRepository;
use crate::application::ports::outbound::message_store::{ConversationRecord, MessageStore};
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
use super::compression_router;
use super::context_compressor;
use super::helpers::{broadcast_event, decr_pending, decr_running, emit_inference_event, incr_running, record_tpm, schedule_cleanup};

// ── Token stream state ──────────────────────────────────────────────────────

struct TokenStreamState {
    token_count: u64,
    text: String,
    /// Buffer for detecting `<think>` tags that may span token boundaries.
    think_buf: String,
    /// True while inside a `<think>…</think>` block — tokens are buffered but
    /// not forwarded to the SSE stream or accumulated into `text`.
    in_think: bool,
    last_owner_refresh: std::time::Instant,
    tool_calls: Vec<serde_json::Value>,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    cached_tokens: Option<u32>,
    ttft_ms: Option<i32>,
}

impl TokenStreamState {
    /// Feed a raw token fragment through the think-block filter.
    /// Returns the portion safe to emit to clients (non-think content).
    /// Maintains `in_think` / `think_buf` state across token boundaries.
    fn consume_text(&mut self, fragment: &str) -> String {
        self.think_buf.push_str(fragment);
        let mut emitted = String::new();
        loop {
            if self.in_think {
                if let Some(pos) = self.think_buf.find("</think>") {
                    self.think_buf.drain(..pos + "</think>".len());
                    self.in_think = false;
                } else {
                    // Still inside <think> — keep only enough bytes to detect a split </think>
                    let keep = self.think_buf.len().saturating_sub(8);
                    if keep > 0 { self.think_buf.drain(..keep); }
                    break;
                }
            } else {
                if let Some(pos) = self.think_buf.find("<think>") {
                    emitted.push_str(&self.think_buf[..pos]);
                    self.think_buf.drain(..pos + "<think>".len());
                    self.in_think = true;
                } else {
                    // No <think> — check for partial tag at end of buffer
                    const TAG: &str = "<think>";
                    let buf_len = self.think_buf.len();
                    let safe_len = (1..TAG.len().min(buf_len + 1))
                        .rev()
                        .find(|&n| {
                            let start = buf_len - n;
                            self.think_buf.is_char_boundary(start)
                                && TAG.starts_with(&self.think_buf[start..])
                        })
                        .map(|n| buf_len - n)
                        .unwrap_or(buf_len);
                    emitted.push_str(&self.think_buf[..safe_len]);
                    self.think_buf.drain(..safe_len);
                    break;
                }
            }
        }
        emitted
    }
}

impl Default for TokenStreamState {
    fn default() -> Self {
        Self {
            token_count: 0,
            text: String::new(),
            think_buf: String::new(),
            in_think: false,
            last_owner_refresh: std::time::Instant::now(),
            tool_calls: Vec::new(),
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            ttft_ms: None,
        }
    }
}

// ── Cancel-resilient S3 persist helper ─────────────────────────────────────
//
// SDD: `.specs/veronex/inference-mcp-streaming-first.md` §6.
//
// CDD invariant (`docs/llm/inference/job-lifecycle.md`): S3 ConversationRecord
// is the SSOT for `result` / `messages` / `tool_calls`. Pre-Tier-B, only the
// happy-path `finalize_job` wrote to S3 — cancel / stream-error paths
// silently dropped accumulated state, leaving DB rows with
// `has_tool_calls=true` but no S3 record (UI: "저장된 결과 없음").
//
// This helper closes that leak. It is called from every terminal exit path
// in `run_job` that has access to a `TokenStreamState` (T2 / T3 / T5 per
// SDD §6.1). The `persisted_to_s3` AtomicBool guard ensures exactly-once
// write across racing finalize_job ↔ cancel paths.
//
// MCP-loop jobs are skipped — bridge owns their S3 write per the existing
// per-conversation append-turns architecture (see
// `bridge.rs::run_loop` post-loop block). Tier-B applies symmetric fix
// there: bridge gates were `if !content.is_empty()` (skipped writes when
// only tool_calls were captured); now `|| !all_mcp_tool_calls.is_empty()`.

/// Append-turn S3 write for the partial state in `ts`. Idempotent via the
/// `persisted_to_s3` guard. Best-effort: errors logged at `warn`, do not
/// propagate (DB row's metadata is the authoritative status).
#[allow(clippy::too_many_arguments)]
async fn persist_partial_conversation(
    message_store: &Option<Arc<dyn MessageStore>>,
    persisted_flag: &AtomicBool,
    job: &InferenceJob,
    ts: &TokenStreamState,
    original_messages: &Option<serde_json::Value>,
    original_prompt: &str,
) {
    // Idempotent guard — only the first caller proceeds.
    if persisted_flag
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    // MCP-loop jobs: bridge writes a single combined turn at end of loop.
    // Skip here to avoid a duplicate per-round turn entry.
    if job.mcp_loop_id.is_some() {
        return;
    }

    let Some(store) = message_store else { return };

    // Skip when no useful state captured (T1: pre-stream cancel; T6:
    // lifecycle_failed before any token).
    if ts.text.is_empty() && ts.tool_calls.is_empty() {
        return;
    }

    let owner_id = job.account_id.or(job.api_key_id).unwrap_or(job.id.0);
    let date = job.created_at.date_naive();
    let s3_key = job.conversation_id.unwrap_or(job.id.0);

    let mut record = match store.get_conversation(owner_id, date, s3_key).await {
        Ok(Some(r)) => r,
        _ => ConversationRecord::new(),
    };

    let result_text = (!ts.text.is_empty())
        .then(|| strip_think_blocks(ts.text.clone()));
    let tool_calls_val = (!ts.tool_calls.is_empty())
        .then(|| serde_json::Value::Array(ts.tool_calls.clone()));

    use crate::application::ports::outbound::message_store::{
        ConversationTurn, TurnRecord,
    };
    record.turns.push(ConversationTurn::Regular(TurnRecord {
        job_id: job.id.0,
        prompt: original_prompt.to_owned(),
        messages: original_messages.clone(),
        tool_calls: tool_calls_val,
        result: result_text,
        model_name: Some(job.model_name.as_str().to_string()),
        created_at: job.created_at.to_rfc3339(),
        compressed: None,
        vision_analysis: None,
    }));

    if let Err(e) = store
        .put_conversation(owner_id, date, s3_key, &record)
        .await
    {
        tracing::warn!(
            job_id = %job.id.0,
            owner_id = %owner_id,
            "S3 partial conversation persist failed: {e}"
        );
    } else {
        tracing::debug!(
            job_id = %job.id.0,
            text_len = ts.text.len(),
            tool_calls = ts.tool_calls.len(),
            "S3 partial conversation persisted (cancel/error path)"
        );
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
    message_store: &Option<Arc<dyn MessageStore>>,
    uuid: Uuid,
    started_at: chrono::DateTime<chrono::Utc>,
    api_key_id: Option<Uuid>,
    tpm_minute: Option<i64>,
    ts: &TokenStreamState,
    error: &anyhow::Error,
    original_messages: &Option<serde_json::Value>,
    original_prompt: &str,
) {
    let msg = error.to_string();

    // T5 (provider stream error): persist whatever tokens were captured before
    // the error. Helper is idempotent + skips MCP-loop jobs.
    let persisted_flag = jobs.get(&uuid).map(|e| e.persisted_to_s3.clone());
    if let Some(flag) = persisted_flag {
        persist_partial_conversation(
            message_store, &flag, job, ts, original_messages, original_prompt,
        ).await;
    }

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

/// Finalize a completed job: write ConversationRecord to S3, persist metrics to
/// Postgres via `finalize()`, broadcast status, record observability.
///
/// Returns `Some(latency_ms)` on normal completion, `None` if cancelled or
/// ownership was lost to another node.
#[allow(clippy::too_many_arguments)]
async fn finalize_job(
    jobs: &Arc<DashMap<Uuid, JobEntry>>,
    job: &mut InferenceJob,
    job_repo: &dyn JobRepository,
    message_store: &Option<Arc<dyn MessageStore>>,
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
    original_messages: Option<serde_json::Value>,
    original_prompt: String,
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

    let result_text = (!ts.text.is_empty()).then_some(strip_think_blocks(ts.text));
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

    // Write turn to S3 ConversationRecord (per-conversation key, append to turns array).
    // MCP loop jobs skip this — the bridge writes a single complete turn after all rounds.
    //
    // SDD §6.2: idempotent guard `persisted_to_s3` prevents this happy-path write
    // from racing the cancel/error helpers (`persist_partial_conversation`).
    let happy_path_persisted_flag = jobs.get(&uuid).map(|e| e.persisted_to_s3.clone());
    let should_write_happy_path = match &happy_path_persisted_flag {
        Some(flag) => flag
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok(),
        None => true, // entry vanished — write defensively, the runner is the authoritative writer
    };
    if should_write_happy_path && let Some(store) = message_store {
        if job.mcp_loop_id.is_none() {
            let owner_id = job.account_id
                .or(job.api_key_id)
                .unwrap_or(uuid);
            let date = job.created_at.date_naive();
            let s3_key = job.conversation_id.unwrap_or(uuid);

            // Read existing record (if any), append this turn
            let mut record = store.get_conversation(owner_id, date, s3_key).await
                .ok().flatten()
                .unwrap_or_else(ConversationRecord::new);

            // Read vision_analysis from in-memory JobEntry (set at submit time).
            let vision_analysis = jobs.get(&uuid).and_then(|e| e.vision_analysis.clone());

            use crate::application::ports::outbound::message_store::{ConversationTurn, TurnRecord as TR};
            record.turns.push(ConversationTurn::Regular(TR {
                job_id: uuid,
                prompt: original_prompt,
                messages: original_messages,
                tool_calls: tool_calls_json.clone(),
                result: result_text.clone(),
                model_name: Some(job.model_name.as_str().to_string()),
                created_at: job.created_at.to_rfc3339(),
                compressed: None,
                vision_analysis,
            }));

            if let Err(e) = store.put_conversation(owner_id, date, s3_key, &record).await {
                tracing::warn!(job_id = %uuid, "S3 conversation write failed (non-fatal): {e}");
            } else if let (Some(conv_id), Some(vk)) = (job.conversation_id, valkey) {
                // Cache the updated record in Valkey (TTL 300 s) so the next read
                // hits cache instead of S3. Compression re-write (Phase 3) will DEL
                // to force a fresh load after the compressed turn is written back.
                const CONV_CACHE_TTL_SECS: i64 = 300;
                let cache_key = crate::domain::constants::conversation_record_key(conv_id);
                if let Ok(json) = serde_json::to_string(&record) {
                    if let Err(e) = vk.kv_set(&cache_key, &json, CONV_CACHE_TTL_SECS, false).await {
                        tracing::warn!(error = %e, "runner: failed to cache conversation record");
                    }
                }

                // Phase 3: spawn per-turn compression (async, non-blocking).
                // Only runs when compression is enabled in lab settings.
                if let Some(handle) = jobs.get(&uuid).and_then(|e| e.compression_handle.clone()) {
                    let store_arc = store.clone();
                    let valkey_arc = Some(vk.clone());
                    tokio::spawn(
                        async move {
                            let lab = handle.lab_settings.get().await.unwrap_or_default();
                            if !lab.context_compression_enabled {
                                return;
                            }
                            let route = compression_router::decide(handle.registry.as_ref(), &lab).await;
                            let model = lab.compression_model.clone()
                                .unwrap_or_else(|| "qwen2.5:3b".to_string());
                            let timeout = lab.compression_timeout_secs as u64;
                            if let Some(params) = route.into_params(model, timeout) {
                                context_compressor::compress_turn(
                                    &params, uuid, owner_id, date, conv_id,
                                    store_arc, valkey_arc,
                                ).await;
                            }
                        }
                        .instrument(tracing::info_span!("veronex.inference.runner.spawn")),
                    );
                }
            }
        }
    }

    // Single terminal Postgres write
    job.status = JobStatus::Completed;
    job.completed_at = Some(completed_at);
    job.result_text = result_text;
    job.tool_calls_json = tool_calls_json;
    job.latency_ms = Some(stored_latency);
    job.ttft_ms = ts.ttft_ms;
    job.prompt_tokens = ts.prompt_tokens.map(|v| v as i32);
    job.completion_tokens = stored_completion;
    job.cached_tokens = ts.cached_tokens.map(|v| v as i32);

    // Result preview: first 20 chars for DB search/listing (full result in S3)
    let result_preview: Option<String> = job.result_text.as_ref()
        .filter(|t| !t.is_empty())
        .map(|t| t.trim_start_matches("/no_think").trim().chars().take(20).collect());

    if let Err(e) = job_repo
        .finalize(
            &job.id,
            job.started_at,
            completed_at,
            job.provider_id,
            job.queue_time_ms,
            stored_latency,
            job.ttft_ms,
            job.prompt_tokens,
            job.completion_tokens,
            job.cached_tokens,
            job.tool_calls_json.is_some(),
            result_preview.as_deref(),
        )
        .await
    {
        tracing::warn!(job_id = %uuid, "failed to finalize job in DB: {e}");
    }

    // Update conversation counters
    if let Some(conv_id) = &job.conversation_id {
        let pt = job.prompt_tokens.unwrap_or(0);
        let ct = job.completion_tokens.unwrap_or(0);
        if let Err(e) = job_repo.update_conversation_counters(conv_id, pt, ct, job.model_name.as_str()).await {
            tracing::warn!(conversation_id = %conv_id, "conversation counter update failed: {e}");
        }
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
    provider: Arc<dyn LlmProviderPort>,
    job_repo: Arc<dyn JobRepository>,
    message_store: Option<Arc<dyn MessageStore>>,
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
    mcp_lifecycle_phase_enabled: bool,
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

    // pending → running: DECR pending, INCR running (no DB write — finalize() handles all)
    decr_pending(&valkey).await;
    incr_running(&valkey).await;

    broadcast_event(&event_tx, &valkey, &instance_id, &JobStatusEvent {
        id: uuid.to_string(),
        status: "running".into(),
        model_name: job.model_name.as_str().into(),
        provider_type: job.provider_type.as_str().into(),
        latency_ms: None,
    }).await;

    // ── Phase 1: Lifecycle (ensure model loaded) ───────────────────────
    // SDD: .specs/veronex/history/inference-lifecycle-sod.md §7.1a.
    // Behind `MCP_LIFECYCLE_PHASE` flag (default off). When on, drives an
    // explicit `ensure_ready` probe on the provider so cold-load timing is
    // observable as its own span / metric instead of being conflated with
    // first-token wait inside stream_tokens.
    if mcp_lifecycle_phase_enabled {
        let lifecycle_started = std::time::Instant::now();
        match provider.ensure_ready(job.model_name.as_str()).await {
            Ok(outcome) => {
                tracing::info!(
                    %uuid,
                    model = %job.model_name.as_str(),
                    ?outcome,
                    duration_ms = lifecycle_started.elapsed().as_millis() as u64,
                    "lifecycle.ensure_ready"
                );
            }
            Err(e) => {
                tracing::warn!(
                    %uuid,
                    model = %job.model_name.as_str(),
                    error = %e,
                    duration_ms = lifecycle_started.elapsed().as_millis() as u64,
                    "lifecycle.ensure_ready failed"
                );
                let job_id = crate::domain::value_objects::JobId(uuid);
                if let Err(re) = job_repo
                    .fail_with_reason(&job_id, "lifecycle_failed", Some(&e.to_string()))
                    .await
                {
                    tracing::warn!(%uuid, "failed to persist lifecycle failure: {re}");
                }
                if let Some(mut entry) = jobs.get_mut(&uuid) {
                    entry.status = JobStatus::Failed;
                    entry.job.status = JobStatus::Failed;
                    entry.job.error = Some(e.to_string());
                    entry.job.failure_reason = Some("lifecycle_failed".into());
                    entry.done = true;
                    let notify = entry.notify.clone();
                    drop(entry);
                    notify.notify_one();
                }
                decr_running(&valkey).await;
                schedule_cleanup(&jobs, uuid, JOB_CLEANUP_DELAY);
                return Ok(None);
            }
        }
    }

    // ── Phase 2: Inference (stream tokens) ─────────────────────────────
    let cancel_notify = jobs.get(&uuid)
        .map(|e| e.cancel_notify.clone())
        .unwrap_or_else(|| Arc::new(Notify::new()));

    // Capture prompt for S3 ConversationRecord at finalize.
    let original_prompt = job.prompt.as_str().to_owned();

    // stream_tokens must be called BEFORE taking messages — the adapter reads
    // job.messages to decide whether to call stream_chat (with tools) or stream_generate.
    let mut stream = provider.stream_tokens(&job);

    // Take messages after stream is started (adapter cloned them into the stream already).
    let original_messages = job.messages.take(); // frees memory, value moved to local
    let mut ts = TokenStreamState::default();

    loop {
        let result = tokio::select! {
            biased;
            _ = cancel_notify.notified() => {
                tracing::info!(%uuid, "job cancelled — dropping stream");
                // T3 — cancel via cancel_notify. Persist whatever was
                // accumulated so far per SDD §6.1.
                let persisted_flag = jobs.get(&uuid).map(|e| e.persisted_to_s3.clone());
                if let Some(flag) = persisted_flag {
                    persist_partial_conversation(
                        &message_store, &flag, &job, &ts,
                        &original_messages, &original_prompt,
                    ).await;
                }
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
            // T2 — cancel observed via in-memory status (set by use_case::cancel
            // racing against the running stream). Persist + exit per SDD §6.1.
            let persisted_flag = entry.persisted_to_s3.clone();
            drop(entry);
            persist_partial_conversation(
                &message_store, &persisted_flag, &job, &ts,
                &original_messages, &original_prompt,
            ).await;
            // running → cancelled: DECR running
            decr_running(&valkey).await;
            return Ok(None);
        }

        match result {
            Ok(mut token) => {
                ts.token_count += 1;
                // Filter <think>…</think> blocks out of the stream.
                // emitted = the non-think portion safe to forward to clients.
                let emitted = if token.value.is_empty() {
                    String::new()
                } else {
                    ts.consume_text(&token.value)
                };
                ts.text.push_str(&emitted);
                // Replace token value with filtered content for SSE forwarding.
                token.value = emitted;

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
                        if let Err(e) = vk.kv_set(&key, instance_id.as_ref(), JOB_OWNER_TTL_SECS, true).await {
                            tracing::warn!(%uuid, error = %e, "runner: failed to refresh job owner TTL");
                        }
                    }
                    ts.last_owner_refresh = std::time::Instant::now();
                }
            }
            Err(e) => {
                drop(entry);
                // T5 — provider stream error mid-stream. handle_stream_error
                // now also persists partial state to S3 (SDD §6.1).
                handle_stream_error(
                    &jobs, &mut job, job_repo.as_ref(), &observability, &valkey,
                    &message_store, uuid, started_at, api_key_id, tpm_minute,
                    &ts, &e, &original_messages, &original_prompt,
                ).await;
                return Err(e);
            }
        }
    }

    // ── Finalize ───────────────────────────────────────────────────────
    let latency_ms = finalize_job(
        &jobs, &mut job, job_repo.as_ref(), &message_store, &valkey, &observability,
        &model_manager, provider_dispatch.as_ref(), &event_tx, &instance_id,
        &cancel_notifiers, uuid, started_at, ts, original_messages, original_prompt,
        api_key_id, tpm_minute, provider_id, provider_is_free_tier,
    ).await;

    Ok(latency_ms)
}

// ── Think-block stripper ──────────────────────────────────────────────────────

/// Remove `<think>…</think>` blocks that reasoning models (Qwen3, DeepSeek-R1,
/// QwQ, etc.) emit as part of their chain-of-thought. These tokens are internal
/// reasoning and must not be stored as the canonical result or included in
/// subsequent conversation context.
///
/// Handles:
/// - Complete blocks: `<think>…</think>`
/// - Unclosed blocks (model cut off mid-think): strips from `<think>` to end
/// - Multiple blocks in one response
/// - Whitespace-only remains are collapsed
pub(super) fn strip_think_blocks(mut text: String) -> String {
    loop {
        let Some(start) = text.find("<think>") else { break };
        if let Some(rel_end) = text[start..].find("</think>") {
            let end = start + rel_end + "</think>".len();
            text.drain(start..end);
        } else {
            // Unclosed — strip from <think> to end of string
            text.truncate(start);
            break;
        }
    }
    // Collapse leading/trailing whitespace introduced by removal
    let trimmed = text.trim();
    if trimmed.len() != text.len() { trimmed.to_string() } else { text }
}

#[cfg(test)]
mod tests {
    use super::TokenStreamState;

    fn collect(tokens: &[&str]) -> String {
        let mut ts = TokenStreamState::default();
        tokens.iter().map(|t| ts.consume_text(t)).collect()
    }

    // ── Regression: multibyte characters must not panic ─────────────────────

    #[test]
    fn multibyte_korean_no_panic() {
        // '안' is 3 bytes — previously caused panic at byte boundary 1
        assert_eq!(collect(&["안녕하세요"]), "안녕하세요");
    }

    #[test]
    fn multibyte_emoji_no_panic() {
        // '😊' is 4 bytes — previously caused panic at byte boundary 2
        assert_eq!(collect(&["Hello 😊"]), "Hello 😊");
    }

    #[test]
    fn multibyte_split_across_tokens_no_panic() {
        // Multibyte chars fed as separate token fragments
        assert_eq!(collect(&["안", "녕"]), "안녕");
    }

    // ── Think-block filtering ────────────────────────────────────────────────

    #[test]
    fn think_block_stripped() {
        assert_eq!(
            collect(&["Hello <think>internal thoughts</think> world"]),
            "Hello  world"
        );
    }

    #[test]
    fn think_block_split_across_tokens() {
        assert_eq!(
            collect(&["He", "llo <thi", "nk>thoughts<", "/think> world"]),
            "Hello  world"
        );
    }

    #[test]
    fn no_think_block_passthrough() {
        assert_eq!(collect(&["Hello world"]), "Hello world");
    }

    #[test]
    fn think_block_with_korean_content() {
        assert_eq!(
            collect(&["안녕 <think>생각중</think> 세계"]),
            "안녕  세계"
        );
    }

    #[test]
    fn emoji_near_think_tag_no_panic() {
        assert_eq!(
            collect(&["😊 <think>thinking</think> done"]),
            "😊  done"
        );
    }

    // ── Tier B — persist_partial_conversation ───────────────────────────────
    //
    // SDD §6.4: `.specs/veronex/inference-mcp-streaming-first.md`. These tests
    // lock the cancel-resilient S3 write contract — every code path that exits
    // `run_job` with accumulated tokens must have its `ts` flushed to S3
    // ConversationRecord (CDD-defined SSOT) exactly once, racing-safe.

    use super::persist_partial_conversation;
    use crate::application::ports::outbound::message_store::{
        ConversationRecord, ConversationTurn, MessageStore,
    };
    use crate::domain::entities::InferenceJob;
    use crate::domain::enums::{ApiFormat, JobStatus, JobSource, ProviderType};
    use crate::domain::value_objects::{JobId, ModelName, Prompt};
    use chrono::NaiveDate;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// In-memory `MessageStore` mock — records every `put_conversation` call
    /// for assertion. Stored records are also retrievable via `get_conversation`
    /// so RMW append-turn flows work in tests.
    struct MockMessageStore {
        puts: Arc<Mutex<Vec<(Uuid, Uuid, ConversationRecord)>>>,
    }

    impl MockMessageStore {
        fn new() -> Self {
            Self { puts: Arc::new(Mutex::new(Vec::new())) }
        }

        fn put_count(&self) -> usize {
            self.puts.lock().unwrap().len()
        }

        fn last_record(&self) -> Option<ConversationRecord> {
            self.puts.lock().unwrap().last().map(|(_, _, r)| {
                ConversationRecord {
                    turns: r.turns.clone(),
                }
            })
        }
    }

    #[async_trait::async_trait]
    impl MessageStore for MockMessageStore {
        async fn put_conversation(
            &self,
            owner_id: Uuid,
            _date: NaiveDate,
            conversation_id: Uuid,
            record: &ConversationRecord,
        ) -> anyhow::Result<()> {
            // Clone via JSON roundtrip (ConversationRecord is not `Clone`).
            let snap: ConversationRecord =
                serde_json::from_str(&serde_json::to_string(record)?)?;
            self.puts.lock().unwrap().push((owner_id, conversation_id, snap));
            Ok(())
        }

        async fn get_conversation(
            &self,
            _owner_id: Uuid,
            _date: NaiveDate,
            _conversation_id: Uuid,
        ) -> anyhow::Result<Option<ConversationRecord>> {
            // Simulate empty backend — every test starts with a fresh record.
            Ok(None)
        }
    }

    fn make_job(mcp_loop_id: Option<Uuid>) -> InferenceJob {
        InferenceJob {
            id: JobId(Uuid::now_v7()),
            account_id: Some(Uuid::now_v7()),
            api_key_id: None,
            provider_id: None,
            provider_type: ProviderType::Ollama,
            model_name: ModelName::new("qwen3-coder-next-200k:latest").unwrap(),
            status: JobStatus::Running,
            source: JobSource::Test,
            prompt: Prompt::new("test").unwrap(),
            prompt_preview: Some("test".into()),
            messages: None,
            tools: None,
            api_format: ApiFormat::OpenaiCompat,
            request_path: None,
            conversation_id: Some(Uuid::now_v7()),
            mcp_loop_id,
            result_text: None,
            tool_calls_json: None,
            error: None,
            failure_reason: None,
            latency_ms: None,
            ttft_ms: None,
            queue_time_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            cancelled_at: None,
            images: None,
            image_keys: None,
            messages_hash: None,
            messages_prefix_hash: None,
            stop: None,
            seed: None,
            response_format: None,
            frequency_penalty: None,
            presence_penalty: None,
            max_tokens: None,
            vision_analysis: None,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    fn flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[tokio::test]
    async fn cancel_after_first_tool_call_persists_to_s3() {
        let store: Arc<dyn MessageStore> = Arc::new(MockMessageStore::new());
        let store_concrete = store.clone();
        let job = make_job(None);
        let mut ts = TokenStreamState::default();
        ts.tool_calls.push(serde_json::json!({"name":"web_search","args":{"q":"micron"}}));
        // text intentionally empty — model only emitted tool_calls before cancel.
        let f = flag();

        persist_partial_conversation(
            &Some(store), &f, &job, &ts, &None, "test prompt",
        ).await;

        let mock = unsafe {
            // Get back to the concrete type for assertions
            &*(Arc::as_ptr(&store_concrete) as *const MockMessageStore)
        };
        assert_eq!(mock.put_count(), 1, "S3 PUT must fire on tool_calls-only state");

        let rec = mock.last_record().unwrap();
        let turn = match &rec.turns[0] {
            ConversationTurn::Regular(t) => t,
            _ => panic!("expected Regular turn"),
        };
        assert!(turn.tool_calls.is_some(), "tool_calls must be persisted");
        assert!(turn.result.is_none(), "result must be None when text empty");
        assert!(f.load(std::sync::atomic::Ordering::Acquire), "flag must be set");
    }

    #[tokio::test]
    async fn cancel_after_partial_text_persists_text_to_s3() {
        let mock = Arc::new(MockMessageStore::new());
        let store: Arc<dyn MessageStore> = mock.clone();
        let job = make_job(None);
        let mut ts = TokenStreamState::default();
        ts.text.push_str("partial answer fragment");
        let f = flag();

        persist_partial_conversation(
            &Some(store), &f, &job, &ts, &None, "test prompt",
        ).await;

        assert_eq!(mock.put_count(), 1);
        let rec = mock.last_record().unwrap();
        let turn = match &rec.turns[0] {
            ConversationTurn::Regular(t) => t,
            _ => panic!(),
        };
        assert_eq!(turn.result.as_deref(), Some("partial answer fragment"));
        assert!(turn.tool_calls.is_none());
    }

    #[tokio::test]
    async fn cancel_before_any_token_skips_s3_write() {
        let mock = Arc::new(MockMessageStore::new());
        let store: Arc<dyn MessageStore> = mock.clone();
        let job = make_job(None);
        let ts = TokenStreamState::default(); // empty
        let f = flag();

        persist_partial_conversation(
            &Some(store), &f, &job, &ts, &None, "test prompt",
        ).await;

        assert_eq!(mock.put_count(), 0, "must not write empty record");
        // Note: flag IS set because compare_exchange runs before the empty check.
        // That's correct — it prevents subsequent finalize from re-attempting.
    }

    #[tokio::test]
    async fn lifecycle_failed_path_skips_s3_write_when_no_tokens() {
        // Equivalent contract to "cancel_before_any_token" but represents the
        // T6 lifecycle_failed entry point (also no tokens collected).
        let mock = Arc::new(MockMessageStore::new());
        let store: Arc<dyn MessageStore> = mock.clone();
        let mut job = make_job(None);
        job.failure_reason = Some("lifecycle_failed".into());
        let ts = TokenStreamState::default();
        let f = flag();

        persist_partial_conversation(
            &Some(store), &f, &job, &ts, &None, "test prompt",
        ).await;

        assert_eq!(mock.put_count(), 0);
    }

    #[tokio::test]
    async fn provider_stream_error_persists_partial_state() {
        // T5 — provider returned Err mid-stream after some tokens accumulated.
        let mock = Arc::new(MockMessageStore::new());
        let store: Arc<dyn MessageStore> = mock.clone();
        let job = make_job(None);
        let mut ts = TokenStreamState::default();
        ts.text.push_str("some text before error");
        ts.tool_calls.push(serde_json::json!({"name":"web_search"}));
        let f = flag();

        persist_partial_conversation(
            &Some(store), &f, &job, &ts, &None, "test prompt",
        ).await;

        assert_eq!(mock.put_count(), 1);
        let rec = mock.last_record().unwrap();
        let turn = rec.turns[0].as_regular_mut_or_panic();
        assert_eq!(turn.result.as_deref(), Some("some text before error"));
        assert!(turn.tool_calls.is_some());
    }

    #[tokio::test]
    async fn parallel_cancel_and_finalize_writes_s3_exactly_once() {
        // SDD §6.2 invariant — `Arc<AtomicBool>` + compare_exchange guarantees
        // exactly-one S3 PUT under racing finalize ↔ cancel paths.
        let mock = Arc::new(MockMessageStore::new());
        let store: Arc<dyn MessageStore> = mock.clone();
        let job = make_job(None);
        let mut ts = TokenStreamState::default();
        ts.text.push_str("answer");
        let f = flag();

        let store_a = Some(store.clone());
        let store_b = Some(store);
        let f_a = f.clone();
        let f_b = f.clone();
        let job_a = job.clone();
        let job_b = job.clone();
        let ts_a = ts_clone(&ts);
        let ts_b = ts_clone(&ts);

        tokio::join!(
            persist_partial_conversation(&store_a, &f_a, &job_a, &ts_a, &None, "p"),
            persist_partial_conversation(&store_b, &f_b, &job_b, &ts_b, &None, "p"),
        );

        assert_eq!(mock.put_count(), 1, "exactly-one write under race");
        assert!(f.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn mcp_loop_jobs_skip_runner_persist() {
        // Bridge owns S3 write for MCP-loop jobs. Runner-side helper must
        // skip them entirely so we don't get a duplicate per-round turn.
        let mock = Arc::new(MockMessageStore::new());
        let store: Arc<dyn MessageStore> = mock.clone();
        let job = make_job(Some(Uuid::now_v7())); // MCP loop
        let mut ts = TokenStreamState::default();
        ts.text.push_str("some text");
        let f = flag();

        persist_partial_conversation(
            &Some(store), &f, &job, &ts, &None, "test",
        ).await;

        assert_eq!(mock.put_count(), 0, "MCP-loop jobs must not be written by runner");
    }

    /// `TokenStreamState` is not `Clone`; `tokio::join` requires owned values
    /// per task. Hand-clone the small set of fields the helper actually reads.
    fn ts_clone(orig: &TokenStreamState) -> TokenStreamState {
        TokenStreamState {
            token_count: orig.token_count,
            text: orig.text.clone(),
            think_buf: orig.think_buf.clone(),
            in_think: orig.in_think,
            last_owner_refresh: orig.last_owner_refresh,
            tool_calls: orig.tool_calls.clone(),
            prompt_tokens: orig.prompt_tokens,
            completion_tokens: orig.completion_tokens,
            cached_tokens: orig.cached_tokens,
            ttft_ms: orig.ttft_ms,
        }
    }

    /// Helper: panic with a clear message if the turn is not Regular.
    /// Defined here because `as_regular_mut` returns `Option`; tests want
    /// fail-loud unwrap with a descriptive message.
    trait AsRegularMutOrPanic {
        fn as_regular_mut_or_panic(&self) -> &crate::application::ports::outbound::message_store::TurnRecord;
    }

    impl AsRegularMutOrPanic for ConversationTurn {
        fn as_regular_mut_or_panic(&self) -> &crate::application::ports::outbound::message_store::TurnRecord {
            match self {
                ConversationTurn::Regular(t) => t,
                _ => panic!("expected Regular turn, got Handoff"),
            }
        }
    }
}
