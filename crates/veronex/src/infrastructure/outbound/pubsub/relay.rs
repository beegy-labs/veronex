//! Cross-instance pub/sub relay for tokens, job events, and cancellation.
//!
//! Uses Valkey pub/sub (via fred `SubscriberClient`) so events produced on
//! instance A are visible on instance B.
//!
//! ## Token streaming
//!
//! Token relay uses **Valkey Streams** (XADD/XREAD) instead of plain pub/sub.
//! This prevents the "initial token black hole" where tokens published before
//! a subscriber connects are permanently lost. Streams persist messages until
//! explicitly trimmed, allowing late-connecting subscribers to catch up.

use std::sync::Arc;

use dashmap::DashMap;
use fred::clients::SubscriberClient;
use fred::interfaces::{EventInterface, PubsubInterface};
use fred::prelude::*;
use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::domain::value_objects::JobStatusEvent;
use crate::infrastructure::outbound::valkey_keys;

// ── Token streaming (Valkey Streams) ────────────────────────────────────────

/// Clean up the token stream key after a job completes.
///
/// Called from `run_job()` completion phase to free Valkey memory.
pub async fn cleanup_token_stream(pool: &Pool, job_id: Uuid) {
    let key = valkey_keys::stream_tokens(job_id);
    if let Err(e) = pool.del::<i64, _>(&key).await {
        tracing::warn!(error = %e, %key, "Valkey DEL token stream cleanup failed");
    }
}

// ── Publisher helpers (Pub/Sub) ─────────────────────────────────────────────

/// Publish a job status event to cross-instance subscribers.
pub async fn publish_job_event(pool: &Pool, event: &JobStatusEvent, instance_id: &str) {
    let payload = serde_json::json!({
        "id": event.id,
        "status": event.status,
        "model_name": event.model_name,
        "provider_type": event.provider_type,
        "latency_ms": event.latency_ms,
        "instance_id": instance_id,
    });
    if let Err(e) = pool
        .next()
        .publish::<i64, _, _>(valkey_keys::pubsub_job_events(), payload.to_string())
        .await
    {
        tracing::warn!(error = %e, "Valkey PUBLISH job_events failed");
    }
}

/// Publish a cancel signal for a job.
pub async fn publish_cancel(pool: &Pool, job_id: Uuid) {
    let channel = valkey_keys::pubsub_cancel(job_id);
    if let Err(e) = pool.next().publish::<i64, _, _>(channel, "cancel".to_string()).await {
        tracing::warn!(error = %e, "Valkey PUBLISH cancel signal failed");
    }
}

// ── Job event subscriber ─────────────────────────────────────────────────────

/// Background task: subscribes to `veronex:pubsub:job_events` and forwards
/// events from other instances to the local broadcast channel.
pub async fn run_job_event_subscriber(
    subscriber: SubscriberClient,
    event_tx: broadcast::Sender<JobStatusEvent>,
    instance_id: Arc<str>,
    shutdown: CancellationToken,
) {
    let mut rx = subscriber.message_rx();

    if let Err(e) = subscriber
        .subscribe(valkey_keys::pubsub_job_events())
        .await
    {
        tracing::error!("failed to subscribe to job events: {e}");
        return;
    }

    tracing::info!("job event subscriber started");

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            msg = rx.recv() => {
                let msg = match msg {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("job event subscriber recv error: {e}");
                        continue;
                    }
                };

                let payload: String = match msg.value.convert() {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let v: serde_json::Value = match serde_json::from_str(&payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Skip our own events.
                if v.get("instance_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id == instance_id.as_ref())
                {
                    continue;
                }

                let event = JobStatusEvent {
                    id: v["id"].as_str().unwrap_or_default().to_string(),
                    status: v["status"].as_str().unwrap_or_default().to_string(),
                    model_name: v["model_name"].as_str().unwrap_or_default().to_string(),
                    provider_type: v["provider_type"].as_str().unwrap_or_default().to_string(),
                    latency_ms: v["latency_ms"].as_i64().map(|v| v as i32),
                };
                let _ = event_tx.send(event);
            }
        }
    }

    let _ = subscriber
        .unsubscribe(valkey_keys::pubsub_job_events())
        .await;
    tracing::info!("job event subscriber stopped");
}

// ── Cancel subscriber ────────────────────────────────────────────────────────

/// Background task: pattern-subscribes to `veronex:pubsub:cancel:*` and fires
/// cancel callbacks for local jobs.
pub async fn run_cancel_subscriber(
    subscriber: SubscriberClient,
    cancel_notifiers: Arc<DashMap<Uuid, Arc<Notify>>>,
    shutdown: CancellationToken,
) {
    let mut rx = subscriber.message_rx();

    if let Err(e) = subscriber
        .psubscribe(valkey_keys::pubsub_cancel_pattern())
        .await
    {
        tracing::error!("failed to psubscribe to cancel channels: {e}");
        return;
    }

    tracing::info!("cancel subscriber started");

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            msg = rx.recv() => {
                let msg = match msg {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("cancel subscriber recv error: {e}");
                        continue;
                    }
                };

                // Extract job_id from channel: veronex:pubsub:cancel:{job_id}
                let channel = msg.channel.to_string();
                let cancel_prefix = valkey_keys::pubsub_cancel_prefix();
                let job_id_str = match channel.strip_prefix(&cancel_prefix) {
                    Some(s) => s,
                    None => continue,
                };
                let job_id = match Uuid::parse_str(job_id_str) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                if let Some(notifier) = cancel_notifiers.get(&job_id) {
                    tracing::info!(%job_id, "cross-instance cancel received");
                    notifier.notify_one();
                }
            }
        }
    }

    let _ = subscriber
        .punsubscribe(valkey_keys::pubsub_cancel_pattern())
        .await;
    tracing::info!("cancel subscriber stopped");
}
