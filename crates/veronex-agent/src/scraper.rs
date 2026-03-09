/// Scrapes node-exporter and Ollama API independently,
/// selecting relevant metrics and forwarding raw values to OTLP.
///
/// Agent policy:
///   - SELECT which metrics to forward (whitelist)
///   - NO value transformation (no unit conversion, no aggregation)
///   - Raw values + original labels → OTLP push
///
/// This ensures `helm install` / `docker-compose up` works without
/// any OTEL Collector configuration changes.
use std::time::Duration;

use serde::Deserialize;

const SCRAPE_TIMEOUT: Duration = Duration::from_secs(5);

/// Metric name prefixes to forward from node-exporter.
/// Everything else is dropped at the agent level.
const NODE_EXPORTER_ALLOWLIST: &[&str] = &[
    "node_memory_MemTotal_bytes",
    "node_memory_MemAvailable_bytes",
    "node_cpu_seconds_total",
    "node_drm_",
    "node_hwmon_temp_celsius",
    "node_hwmon_power_average_watt",
    "node_hwmon_chip_names",
];

fn is_allowed(name: &str) -> bool {
    NODE_EXPORTER_ALLOWLIST.iter().any(|prefix| name.starts_with(prefix))
}

/// A single gauge metric — raw name, raw value, raw labels from the source.
pub struct Gauge {
    pub name: String,
    pub value: f64,
    pub labels: Vec<(String, String)>,
}

// ── Node-exporter ────────────────────────────────────────────────────────────

/// Scrape node-exporter /metrics — select allowed metrics, forward raw values.
pub async fn scrape_node_exporter(client: &reqwest::Client, base_url: &str) -> Vec<Gauge> {
    let url = format!("{}/metrics", base_url.trim_end_matches('/'));
    match client.get(&url).timeout(SCRAPE_TIMEOUT).send().await {
        Ok(resp) => match resp.text().await {
            Ok(text) => parse_node_exporter(&text),
            Err(e) => {
                tracing::warn!("node-exporter read failed: {e}");
                vec![]
            }
        },
        Err(e) => {
            tracing::warn!("node-exporter scrape failed: {e}");
            vec![]
        }
    }
}

/// Parse Prometheus text — filter by allowlist, pass raw values through.
fn parse_node_exporter(text: &str) -> Vec<Gauge> {
    let mut gauges = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((metric_part, value)) = parse_line(line) else {
            continue;
        };

        let name = match metric_part.find('{') {
            Some(i) => &metric_part[..i],
            None => metric_part,
        };

        if !is_allowed(name) {
            continue;
        }

        gauges.push(Gauge {
            name: name.to_string(),
            value,
            labels: parse_labels(metric_part),
        });
    }

    gauges
}

/// Parse a Prometheus metric line into (metric_part, value).
fn parse_line(line: &str) -> Option<(&str, f64)> {
    let split_at = if let Some(brace_end) = line.find('}') {
        line[brace_end..].find(' ').map(|i| brace_end + i)
    } else {
        line.find(' ')
    }?;
    let metric_part = &line[..split_at];
    let value: f64 = line[split_at + 1..].split_whitespace().next()?.parse().ok()?;
    Some((metric_part, value))
}

/// Extract all labels from a Prometheus metric part: `name{k1="v1",k2="v2"}`.
fn parse_labels(metric_part: &str) -> Vec<(String, String)> {
    let Some(start) = metric_part.find('{') else {
        return vec![];
    };
    let Some(end) = metric_part.find('}') else {
        return vec![];
    };
    let inner = &metric_part[start + 1..end];
    let mut labels = Vec::new();
    let mut rest = inner;

    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = &rest[..eq];
        let after_eq = &rest[eq + 1..];

        if !after_eq.starts_with('"') {
            break;
        }
        let Some(val_end) = after_eq[1..].find('"') else {
            break;
        };
        let val = &after_eq[1..1 + val_end];
        labels.push((key.to_string(), val.to_string()));

        let consumed = eq + 1 + 1 + val_end + 1;
        rest = &rest[consumed..];
        if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }

    labels
}

// ── Ollama ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OllamaPsResponse {
    models: Option<Vec<OllamaPsModel>>,
}

#[derive(Deserialize)]
struct OllamaPsModel {
    name: Option<String>,
    size_vram: Option<u64>,
    size: Option<u64>,
}

/// Scrape Ollama /api/ps — forward raw byte values, no conversion.
pub async fn scrape_ollama(client: &reqwest::Client, base_url: &str) -> Vec<Gauge> {
    let url = format!("{}/api/ps", base_url.trim_end_matches('/'));
    let resp: OllamaPsResponse = match client.get(&url).timeout(SCRAPE_TIMEOUT).send().await {
        Ok(r) => match r.json().await {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("ollama parse failed: {e}");
                return vec![];
            }
        },
        Err(e) => {
            tracing::debug!("ollama scrape failed: {e}");
            return vec![];
        }
    };

    let models = resp.models.unwrap_or_default();
    let mut gauges = Vec::with_capacity(models.len() * 2 + 1);

    gauges.push(Gauge {
        name: "ollama_loaded_models".into(),
        value: models.len() as f64,
        labels: vec![],
    });

    for model in &models {
        let model_label = vec![(
            "model".into(),
            model.name.as_deref().unwrap_or("unknown").to_string(),
        )];

        if let Some(vram) = model.size_vram {
            gauges.push(Gauge {
                name: "ollama_model_size_vram_bytes".into(),
                value: vram as f64,
                labels: model_label.clone(),
            });
        }
        if let Some(size) = model.size {
            gauges.push(Gauge {
                name: "ollama_model_size_bytes".into(),
                value: size as f64,
                labels: model_label,
            });
        }
    }

    gauges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_by_allowlist() {
        let text = r#"
node_memory_MemTotal_bytes 6.7385810944e+10
node_filesystem_size_bytes{mountpoint="/"} 500000000000
node_drm_gpu_busy_percent{card="card1"} 42
some_random_metric 999
"#;
        let gauges = parse_node_exporter(text);
        assert_eq!(gauges.len(), 2); // only memory + drm
        assert!(gauges.iter().any(|g| g.name == "node_memory_MemTotal_bytes"));
        assert!(gauges.iter().any(|g| g.name == "node_drm_gpu_busy_percent"));
    }

    #[test]
    fn raw_values_no_conversion() {
        let text = "node_memory_MemTotal_bytes 6.7385810944e+10\n";
        let gauges = parse_node_exporter(text);
        let total = gauges.iter().find(|g| g.name == "node_memory_MemTotal_bytes").unwrap();
        // Raw bytes — NOT converted to MB
        assert!((total.value - 6.7385810944e10).abs() < 1.0);
    }

    #[test]
    fn preserves_labels() {
        let text = r#"node_hwmon_temp_celsius{chip="0000:c4:00_0",sensor="temp1"} 55"#;
        let gauges = parse_node_exporter(text);
        assert_eq!(gauges.len(), 1);
        assert_eq!(gauges[0].labels, vec![
            ("chip".into(), "0000:c4:00_0".into()),
            ("sensor".into(), "temp1".into()),
        ]);
    }

    #[test]
    fn cpu_lines_passed_individually() {
        let text = r#"
node_cpu_seconds_total{cpu="0",mode="idle"} 1000
node_cpu_seconds_total{cpu="0",mode="user"} 500
node_cpu_seconds_total{cpu="1",mode="idle"} 900
"#;
        let gauges = parse_node_exporter(text);
        // 3 individual lines — NOT aggregated into cpu count
        assert_eq!(gauges.len(), 3);
        assert!(gauges.iter().all(|g| g.name == "node_cpu_seconds_total"));
    }

    #[test]
    fn parse_labels_multiple() {
        let labels = parse_labels(r#"foo{cpu="3",mode="idle"}"#);
        assert_eq!(labels, vec![
            ("cpu".into(), "3".into()),
            ("mode".into(), "idle".into()),
        ]);
    }

    #[test]
    fn parse_labels_empty() {
        let labels = parse_labels("foo");
        assert!(labels.is_empty());
    }
}
