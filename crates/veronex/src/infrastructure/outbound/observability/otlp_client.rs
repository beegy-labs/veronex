/// Minimal OTLP HTTP/JSON log emitter.
///
/// Sends log records to the OTel Collector's `/v1/logs` endpoint.
/// Schema is intentionally identical to the one produced by `veronex-analytics`
/// so that the ClickHouse Kafka Engine consumers require no changes.
///
/// All failures are logged as warnings and discarded (fail-open).
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct OtlpClient {
    client: reqwest::Client,
    /// Full URL including path, e.g. `http://otel-collector:4318/v1/logs`
    endpoint: String,
}

impl OtlpClient {
    pub fn new(base_url: &str) -> Self {
        let endpoint = format!("{}/v1/logs", base_url.trim_end_matches('/'));
        tracing::info!("OTLP logs endpoint (veronex direct): {endpoint}");
        Self {
            client: reqwest::Client::new(),
            endpoint,
        }
    }

    /// Emit a single OTLP log record. Errors are logged and discarded.
    ///
    /// `service_name` is embedded in the resource attributes so the OTel Collector
    /// can route records correctly (same pipeline as veronex-analytics).
    pub async fn emit(
        &self,
        body: &str,
        event_time: DateTime<Utc>,
        attributes: Vec<(&'static str, Value)>,
    ) {
        let time_ns = event_time
            .timestamp_nanos_opt()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string());

        let observed_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_string());

        let attrs: Vec<Value> = attributes
            .into_iter()
            .map(|(k, v)| json!({"key": k, "value": v}))
            .collect();

        let payload = json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": [
                        {"key": "service.name", "value": {"stringValue": "veronex"}}
                    ]
                },
                "scopeLogs": [{
                    "scope": {"name": "veronex"},
                    "logRecords": [{
                        "timeUnixNano": time_ns,
                        "observedTimeUnixNano": observed_ns,
                        "severityNumber": 9,
                        "severityText": "INFO",
                        "body": {"stringValue": body},
                        "attributes": attrs
                    }]
                }]
            }]
        });

        if let Err(e) = self
            .client
            .post(&self.endpoint)
            .json(&payload)
            .send()
            .await
        {
            tracing::warn!("OTLP emit failed (fail-open): {e}");
        }
    }
}
