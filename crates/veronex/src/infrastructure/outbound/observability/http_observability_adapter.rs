use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};

/// HTTP adapter that forwards inference events to `veronex-analytics`
/// via `POST /internal/ingest/inference`.
///
/// Fail-open: errors are logged as warnings and swallowed.
pub struct HttpObservabilityAdapter {
    http: reqwest::Client,
    analytics_url: String,
    secret: String,
}

impl HttpObservabilityAdapter {
    pub fn new(analytics_url: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            analytics_url: analytics_url.into(),
            secret: secret.into(),
        }
    }
}

#[derive(Serialize)]
struct IngestInferencePayload<'a> {
    pub event_time: chrono::DateTime<Utc>,
    pub request_id: uuid::Uuid,
    pub api_key_id: Option<uuid::Uuid>,
    pub tenant_id: &'a str,
    pub model_name: &'a str,
    pub provider_type: &'a str,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub latency_ms: u32,
    pub finish_reason: String,
    pub status: &'a str,
    pub error_msg: Option<&'a str>,
}

#[async_trait]
impl ObservabilityPort for HttpObservabilityAdapter {
    async fn record_inference(&self, event: &InferenceEvent) -> Result<()> {
        let finish_reason = format!("{:?}", event.finish_reason).to_lowercase();
        let payload = IngestInferencePayload {
            event_time: event.event_time,
            request_id: event.request_id,
            api_key_id: event.api_key_id,
            tenant_id: &event.tenant_id,
            model_name: &event.model_name,
            provider_type: &event.provider_type,
            prompt_tokens: event.prompt_tokens,
            completion_tokens: event.completion_tokens,
            latency_ms: event.latency_ms,
            finish_reason,
            status: &event.status,
            error_msg: event.error_msg.as_deref(),
        };

        let url = format!("{}/internal/ingest/inference", self.analytics_url);
        if let Err(e) = self
            .http
            .post(&url)
            .bearer_auth(&self.secret)
            .json(&payload)
            .send()
            .await
        {
            tracing::warn!(
                request_id = %event.request_id,
                "analytics ingest inference failed (non-fatal): {e}"
            );
        }

        Ok(())
    }
}
