use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::entities::InferenceJob;
use crate::domain::enums::FinishReason;
use crate::domain::value_objects::JobStatusEvent;
use crate::domain::constants::TPM_ESTIMATED_TOKENS;

use super::JobEntry;

// ── Event broadcasting ──────────────────────────────────────────────────────

pub(super) async fn broadcast_event(
    event_tx: &broadcast::Sender<JobStatusEvent>,
    valkey: &Option<Arc<dyn ValkeyPort>>,
    instance_id: &str,
    event: &JobStatusEvent,
) {
    let _ = event_tx.send(event.clone());
    if let Some(vk) = valkey {
        vk.publish_job_event(event, instance_id).await;
    }
}

// ── Deferred cleanup ────────────────────────────────────────────────────────

pub(super) fn schedule_cleanup(
    jobs: &Arc<DashMap<Uuid, JobEntry>>,
    uuid: Uuid,
    delay: std::time::Duration,
) {
    let jobs = jobs.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        jobs.remove(&uuid);
    });
}

// ── Observability event ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn emit_inference_event(
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
    let Some(obs) = observability else { return };

    let event = InferenceEvent {
        event_time: chrono::Utc::now(),
        request_id: uuid,
        api_key_id,
        tenant_id: String::new(),
        model_name: job.model_name.as_str().to_string(),
        provider_type: job.provider_type.as_str().to_string(),
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

// ── TPM accounting ──────────────────────────────────────────────────────────

/// Adjust TPM counter: actual tokens minus the reservation made at admission.
pub async fn record_tpm(
    valkey: &dyn ValkeyPort,
    api_key_id: Uuid,
    tokens: u64,
    reservation_minute: Option<i64>,
) -> anyhow::Result<()> {
    let adjustment = tokens as i64 - TPM_ESTIMATED_TOKENS;
    if adjustment == 0 {
        return Ok(());
    }

    let minute = reservation_minute.unwrap_or_else(|| chrono::Utc::now().timestamp() / 60);
    let key = crate::domain::constants::ratelimit_tpm_key(api_key_id, minute);
    valkey.incr_by(&key, adjustment).await?;
    Ok(())
}
