use anyhow::Result;
use async_trait::async_trait;
use clickhouse::Row;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};

/// A single row in the `inference_logs` ClickHouse table.
#[derive(Row, Serialize, Debug)]
struct InferenceLogRow {
    /// Unix timestamp in milliseconds (DateTime64(3)).
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    event_time: OffsetDateTime,
    api_key_id: uuid::Uuid,
    tenant_id: String,
    request_id: uuid::Uuid,
    model_name: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    latency_ms: u32,
    finish_reason: String,
    status: String,
}

/// ClickHouse-backed implementation of [`ObservabilityPort`].
///
/// Inserts one row into `inference_logs` per inference event.
/// All errors are logged as warnings and swallowed — observability failures
/// must not affect inference results.
pub struct ClickHouseObservabilityAdapter {
    client: clickhouse::Client,
}

impl ClickHouseObservabilityAdapter {
    pub fn new(client: clickhouse::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ObservabilityPort for ClickHouseObservabilityAdapter {
    async fn record_inference(&self, event: &InferenceEvent) -> Result<()> {
        let finish_reason = format!("{:?}", event.finish_reason).to_lowercase();

        // Convert chrono::DateTime<Utc> → time::OffsetDateTime
        let ts_nanos = event.event_time.timestamp_nanos_opt().unwrap_or(0);
        let event_time = OffsetDateTime::from_unix_timestamp_nanos(ts_nanos as i128)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);

        let row = InferenceLogRow {
            event_time,
            api_key_id: event.api_key_id.unwrap_or_else(Uuid::nil),
            tenant_id: event.tenant_id.clone(),
            request_id: event.request_id,
            model_name: event.model_name.clone(),
            prompt_tokens: event.prompt_tokens,
            completion_tokens: event.completion_tokens,
            latency_ms: event.latency_ms,
            finish_reason,
            status: event.status.clone(),
        };

        let mut insert = match self.client.insert("inference_logs") {
            Ok(ins) => ins,
            Err(e) => {
                tracing::warn!(
                    request_id = %event.request_id,
                    "clickhouse insert init failed (non-fatal): {e}"
                );
                return Ok(());
            }
        };

        if let Err(e) = insert.write(&row).await {
            tracing::warn!(
                request_id = %event.request_id,
                "clickhouse write failed (non-fatal): {e}"
            );
            return Ok(());
        }

        if let Err(e) = insert.end().await {
            tracing::warn!(
                request_id = %event.request_id,
                "clickhouse insert end failed (non-fatal): {e}"
            );
        }

        Ok(())
    }
}
