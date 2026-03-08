/// Lightweight OTLP HTTP/JSON client.
///
/// Sends log records to the OTel Collector's HTTP endpoint (`/v1/logs`).
/// All failures are logged as warnings and ignored (fail-open).
use chrono::{DateTime, Utc};
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

    /// Return the resolved OTLP endpoint URL (for testing).
    #[cfg(test)]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Emit a single log record.  Non-blocking — errors are logged and discarded.
    ///
    /// * `event_time` — original event timestamp used as `timeUnixNano`.
    /// * `observedTimeUnixNano` is always set to the current wall-clock time
    ///   (when the collector received the event).
    pub async fn emit(
        &self,
        body: &str,
        event_time: DateTime<Utc>,
        attributes: Vec<(&str, Value)>,
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
                        {"key": "service.name", "value": {"stringValue": "veronex-analytics"}}
                    ]
                },
                "scopeLogs": [{
                    "scope": {"name": "veronex-analytics"},
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

        if let Err(e) = self.client.post(&self.endpoint).json(&payload).send().await {
            tracing::warn!("OTLP emit failed (fail-open): {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_appends_v1_logs_path() {
        let client = OtlpClient::new("http://collector:4318");
        assert_eq!(client.endpoint(), "http://collector:4318/v1/logs");
    }

    #[test]
    fn new_strips_trailing_slash() {
        let client = OtlpClient::new("http://collector:4318/");
        assert_eq!(client.endpoint(), "http://collector:4318/v1/logs");
    }

    #[test]
    fn new_handles_multiple_trailing_slashes() {
        // trim_end_matches removes all trailing slashes
        let client = OtlpClient::new("http://collector:4318///");
        assert_eq!(client.endpoint(), "http://collector:4318/v1/logs");
    }
}
