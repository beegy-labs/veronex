/// Lightweight OTLP HTTP/JSON client for pushing gauge metrics.
///
/// Constructs an ExportMetricsServiceRequest in OTLP JSON format and POSTs
/// it to the collector's `/v1/metrics` endpoint.  Fail-open: errors are
/// returned but never fatal.
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{json, Value};

use crate::scraper::Gauge;

/// Push a batch of gauge metrics to the OTel Collector via OTLP HTTP.
pub async fn push_metrics(
    client: &reqwest::Client,
    otel_endpoint: &str,
    server_id: &str,
    server_name: &str,
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

    let metrics: Vec<Value> = gauges
        .iter()
        .map(|g| {
            let attrs: Vec<Value> = g
                .labels
                .iter()
                .map(|(k, v)| json!({"key": k, "value": {"stringValue": v}}))
                .collect();

            json!({
                "name": g.name,
                "unit": unit_for(g.name),
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
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "veronex-agent"}},
                    {"key": "server.id", "value": {"stringValue": server_id}},
                    {"key": "server.name", "value": {"stringValue": server_name}}
                ]
            },
            "scopeMetrics": [{
                "scope": {"name": "veronex-agent"},
                "metrics": metrics
            }]
        }]
    });

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OTLP push failed: {status} — {body}");
    }

    Ok(())
}

/// Map metric name to OTel unit string.
fn unit_for(name: &str) -> &'static str {
    match name {
        "system.memory.total" | "system.memory.used" | "gpu.vram.used" | "gpu.vram.total"
        | "ollama.model.vram" => "MiBy",
        "gpu.utilization" => "%",
        "gpu.temperature" => "Cel",
        "gpu.power" => "W",
        "system.cpu.count" | "ollama.loaded_models" => "{count}",
        _ => "",
    }
}
