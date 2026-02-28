use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use rskafka::{
    client::{
        partition::{Compression, UnknownTopicHandling},
        ClientBuilder,
    },
    record::Record,
};
use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};

/// Redpanda-backed implementation of [`ObservabilityPort`].
///
/// Serialises each [`InferenceEvent`] to flat JSON and produces it onto the
/// `inference` topic.  ClickHouse consumes this topic via a Kafka Engine →
/// Materialized View chain and writes rows into `inference_logs`.
///
/// All errors are logged as warnings and swallowed — observability failures
/// must not affect inference results.
pub struct RedpandaObservabilityAdapter {
    partition_client: Arc<rskafka::client::partition::PartitionClient>,
}

impl RedpandaObservabilityAdapter {
    pub async fn new(brokers: Vec<String>) -> Result<Self> {
        let client = ClientBuilder::new(brokers).build().await?;
        let partition_client = client
            .partition_client("inference", 0, UnknownTopicHandling::Retry)
            .await?;
        Ok(Self {
            partition_client: Arc::new(partition_client),
        })
    }
}

#[async_trait]
impl ObservabilityPort for RedpandaObservabilityAdapter {
    async fn record_inference(&self, event: &InferenceEvent) -> Result<()> {
        let event_time_ms = event.event_time.timestamp_millis();
        let finish_reason = format!("{:?}", event.finish_reason).to_lowercase();
        let api_key_id = event
            .api_key_id
            .map(|id| id.to_string())
            .unwrap_or_default();

        let payload = serde_json::json!({
            "event_time_ms":      event_time_ms,
            "api_key_id":         api_key_id,
            "tenant_id":          event.tenant_id,
            "request_id":         event.request_id.to_string(),
            "model_name":         event.model_name,
            "prompt_tokens":      event.prompt_tokens,
            "completion_tokens":  event.completion_tokens,
            "latency_ms":         event.latency_ms,
            "finish_reason":      finish_reason,
            "status":             event.status,
        });

        let json_bytes = serde_json::to_vec(&payload)
            .map_err(|e| anyhow::anyhow!("json serialization failed: {e}"))?;

        let record = Record {
            key: None,
            value: Some(json_bytes.into()),
            headers: BTreeMap::new(),
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = self
            .partition_client
            .produce(vec![record], Compression::NoCompression)
            .await
        {
            tracing::warn!(
                request_id = %event.request_id,
                "redpanda produce failed (non-fatal): {e}"
            );
        }

        Ok(())
    }
}
