use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};

use super::otlp_client::OtlpClient;

/// OTLP adapter that emits inference events directly to the OTel Collector
/// via `POST /v1/logs` (OTLP HTTP/JSON).
///
/// Replaces the two-hop `veronex → veronex-analytics → OTel Collector` path
/// with a single hop: `veronex → OTel Collector → Kafka → Redpanda → ClickHouse`.
///
/// The emitted log record schema is identical to the one produced by
/// `veronex-analytics::handlers::ingest::ingest_inference`, so ClickHouse
/// Kafka Engine consumers require no changes.
///
/// Fail-open: errors are logged as warnings and swallowed.
pub struct OtlpObservabilityAdapter {
    otlp: OtlpClient,
}

impl OtlpObservabilityAdapter {
    pub fn new(otel_http_endpoint: &str) -> Self {
        Self {
            otlp: OtlpClient::new(otel_http_endpoint),
        }
    }
}

#[async_trait]
impl ObservabilityPort for OtlpObservabilityAdapter {
    async fn record_inference(&self, event: &InferenceEvent) -> Result<()> {
        let mut attrs = vec![
            ("event.name", json!({"stringValue": "inference.completed"})),
            ("request_id", json!({"stringValue": event.request_id.to_string()})),
            ("tenant_id", json!({"stringValue": event.tenant_id})),
            ("model_name", json!({"stringValue": event.model_name})),
            ("provider_type", json!({"stringValue": event.provider_type})),
            ("prompt_tokens", json!({"intValue": event.prompt_tokens.to_string()})),
            ("completion_tokens", json!({"intValue": event.completion_tokens.to_string()})),
            ("latency_ms", json!({"intValue": event.latency_ms.to_string()})),
            ("finish_reason", json!({"stringValue": event.finish_reason.as_str()})),
            ("status", json!({"stringValue": event.status})),
        ];

        if let Some(id) = event.api_key_id {
            attrs.push(("api_key_id", json!({"stringValue": id.to_string()})));
        }
        if let Some(ref msg) = event.error_msg {
            attrs.push(("error_msg", json!({"stringValue": msg})));
        }

        self.otlp
            .emit("inference.completed", event.event_time, attrs)
            .await;

        Ok(())
    }
}
