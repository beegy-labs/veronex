/// Lightweight OTLP HTTP/JSON client for pushing raw gauge metrics.
/// No transformation — just format conversion (Gauge → OTLP JSON).
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{json, Value};

use crate::scraper::Gauge;

const OTLP_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);

/// Push a batch of gauge metrics to the OTel Collector via OTLP HTTP.
/// Retries once with 5s backoff on failure.
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

            json!({
                "name": g.name,
                "gauge": {
                    "dataPoints": [{
                        "asDouble": g.value,
                        "timeUnixNano": &now_ns,
                        "attributes": attrs
                    }]
                }
            })
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

    for attempt in 0..2 {
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
                if attempt == 0 {
                    tracing::debug!(status = %status, "OTLP push failed, retrying in 5s: {body}");
                    tokio::time::sleep(OTLP_RETRY_BACKOFF).await;
                } else {
                    anyhow::bail!("OTLP push failed after retry: {status} — {body}");
                }
            }
            Err(e) => {
                if attempt == 0 {
                    tracing::debug!("OTLP push error, retrying in 5s: {e}");
                    tokio::time::sleep(OTLP_RETRY_BACKOFF).await;
                } else {
                    anyhow::bail!("OTLP push failed after retry: {e}");
                }
            }
        }
    }

    Ok(())
}
