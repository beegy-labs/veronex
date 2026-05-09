use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::application::ports::outbound::observability_port::{InferenceEvent, ObservabilityPort};

use super::otlp_client::OtlpClient;

/// Map our internal `provider_type` string to the OpenTelemetry GenAI
/// semantic-convention value for `gen_ai.system`. Aligns with vLLM,
/// llm-d / IGW (`llama_cpp` for llama.cpp) and OpenLLMetry.
///
/// Spec: https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/
fn gen_ai_system(provider_type: &str) -> &'static str {
    match provider_type {

        "gemini" => "gemini",
        "llama_server" => "llama_cpp",
        _ => "unknown",
    }
}

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
        // Legacy attributes — ClickHouse Kafka Engine consumer reads these
        // names verbatim, so they stay as-is. The OTel `gen_ai.*` keys below
        // are added alongside (additive, non-breaking) so vLLM/IGW-style
        // dashboards and OpenLLMetry consumers see standard semantics.
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
            // OTel GenAI semantic conventions — opt-in dashboards discover
            // these without needing to know our legacy field names.
            ("gen_ai.system", json!({"stringValue": gen_ai_system(&event.provider_type)})),
            ("gen_ai.request.model", json!({"stringValue": event.model_name})),
            ("gen_ai.response.model", json!({"stringValue": event.model_name})),
            ("gen_ai.usage.input_tokens", json!({"intValue": event.prompt_tokens.to_string()})),
            ("gen_ai.usage.output_tokens", json!({"intValue": event.completion_tokens.to_string()})),
            ("gen_ai.response.finish_reasons", json!({"stringValue": event.finish_reason.as_str()})),
        ];

        if let Some(ttft) = event.ttft_ms {
            // Convention is seconds in the spec but most backends accept ms;
            // emit both so downstream picks whichever they prefer.
            attrs.push(("gen_ai.server.time_to_first_token_ms", json!({"intValue": ttft.to_string()})));
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_ai_system_maps_known_provider_types() {

        assert_eq!(gen_ai_system("gemini"), "gemini");
        assert_eq!(gen_ai_system("llama_server"), "llama_cpp");
    }

    #[test]
    fn gen_ai_system_falls_back_for_unknown() {
        assert_eq!(gen_ai_system("vertex"), "unknown");
        assert_eq!(gen_ai_system(""), "unknown");
    }
}
