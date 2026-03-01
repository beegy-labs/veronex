/// Lightweight OTLP HTTP/JSON client.
///
/// Sends log records to the OTel Collector's HTTP endpoint (`/v1/logs`).
/// All failures are logged as warnings and ignored (fail-open).
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct OtlpClient {
    client: Client,
    /// Full URL including path, e.g. `http://otel-collector:4318/v1/logs`
    endpoint: String,
}

impl OtlpClient {
    pub fn new(base_url: &str) -> Self {
        let endpoint = format!("{}/v1/logs", base_url.trim_end_matches('/'));
        tracing::info!("OTLP HTTP logs endpoint: {endpoint}");
        Self {
            client: Client::new(),
            endpoint,
        }
    }

    /// Emit a single log record.  Non-blocking — errors are logged and discarded.
    pub async fn emit(&self, body: &str, attributes: Vec<(&str, Value)>) {
        let now_ns = SystemTime::now()
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
                        {"key": "service.name", "value": {"stringValue": "veronex-analytics"}}
                    ]
                },
                "scopeLogs": [{
                    "scope": {"name": "veronex-analytics"},
                    "logRecords": [{
                        "timeUnixNano": now_ns,
                        "observedTimeUnixNano": now_ns,
                        "severityNumber": 9,
                        "severityText": "INFO",
                        "body": {"stringValue": body},
                        "attributes": attrs
                    }]
                }]
            }]
        });

        if let Err(e) = self.client.post(&self.endpoint).json(&payload).send().await {
            tracing::warn!("OTLP emit failed (fail-open): {e}");
        }
    }
}
