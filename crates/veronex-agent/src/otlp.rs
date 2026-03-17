/// Lightweight OTLP HTTP/JSON client for pushing metrics (gauges + counters).
/// Counters (e.g. node_cpu_seconds_total) are sent as OTLP Sum with
/// isMonotonic=true so downstream systems can compute correct deltas.
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{json, Value};

use crate::scraper::Gauge;

/// Metric names that are monotonic counters (not gauges).
/// Sent as OTLP `sum` with `isMonotonic: true`.
const COUNTER_NAMES: &[&str] = &["node_cpu_seconds_total"];

fn is_counter(name: &str) -> bool {
    COUNTER_NAMES.contains(&name)
}

/// Max retry attempts for OTLP push.
const MAX_RETRIES: u32 = 3;

/// Push a batch of metrics to the OTel Collector via OTLP HTTP.
/// Retries up to 3 times with exponential backoff (2s, 4s, 8s).
pub async fn push_metrics(
    client: &reqwest::Client,
    otel_endpoint: &str,
    labels: &HashMap<String, String>,
    gauges: &[Gauge],
) -> Result<()> {
    if gauges.is_empty() {
        return Ok(());
    }

    let url = format!("{}/v1/metrics", otel_endpoint.trim_end_matches('/'));
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".into());

    let resource_attrs: Vec<Value> = labels
        .iter()
        .map(|(k, v)| json!({"key": k, "value": {"stringValue": v}}))
        .chain(std::iter::once(json!({"key": "service.name", "value": {"stringValue": "veronex-agent"}})))
        .collect();

    let metrics: Vec<Value> = gauges
        .iter()
        .map(|g| {
            let attrs: Vec<Value> = g.labels
                .iter()
                .map(|(k, v)| json!({"key": k, "value": {"stringValue": v}}))
                .collect();

            let dp = json!({
                "asDouble": g.value,
                "timeUnixNano": &now_ns,
                "attributes": attrs
            });

            if is_counter(&g.name) {
                json!({
                    "name": g.name,
                    "sum": {
                        "dataPoints": [dp],
                        "aggregationTemporality": 2,
                        "isMonotonic": true
                    }
                })
            } else {
                json!({
                    "name": g.name,
                    "gauge": {
                        "dataPoints": [dp]
                    }
                })
            }
        })
        .collect();

    let payload = json!({
        "resourceMetrics": [{
            "resource": { "attributes": resource_attrs },
            "scopeMetrics": [{
                "scope": {"name": "veronex-agent"},
                "metrics": metrics
            }]
        }]
    });

    for attempt in 0..MAX_RETRIES {
        let result = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_else(|_| "<unreadable>".into());
                if attempt + 1 < MAX_RETRIES {
                    let backoff = std::time::Duration::from_secs(2u64 << attempt);
                    tracing::debug!(status = %status, attempt, "OTLP push failed, retrying in {backoff:?}: {body}");
                    tokio::time::sleep(backoff).await;
                } else {
                    anyhow::bail!("OTLP push failed after {MAX_RETRIES} attempts: {status} — {body}");
                }
            }
            Err(e) => {
                if attempt + 1 < MAX_RETRIES {
                    let backoff = std::time::Duration::from_secs(2u64 << attempt);
                    tracing::debug!(attempt, "OTLP push error, retrying in {backoff:?}: {e}");
                    tokio::time::sleep(backoff).await;
                } else {
                    anyhow::bail!("OTLP push failed after {MAX_RETRIES} attempts: {e}");
                }
            }
        }
    }

    Ok(())
}
