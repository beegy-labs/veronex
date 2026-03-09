/// Scrapes node-exporter (Prometheus text) and Ollama API independently,
/// converting raw data to OTLP gauges WITHOUT any transformation.
///
/// Agent responsibility: collect + forward only.
/// Filtering, unit conversion, aggregation → OTEL Collector transform processor.
use std::time::Duration;

use serde::Deserialize;

const SCRAPE_TIMEOUT: Duration = Duration::from_secs(5);

/// A single gauge metric — raw name, raw value, raw labels from the source.
pub struct Gauge {
    pub name: String,
    pub value: f64,
    pub labels: Vec<(String, String)>,
}

// ── Node-exporter ────────────────────────────────────────────────────────────

/// Scrape node-exporter /metrics and return every Prometheus line as a Gauge.
pub async fn scrape_node_exporter(client: &reqwest::Client, base_url: &str) -> Vec<Gauge> {
    let url = format!("{}/metrics", base_url.trim_end_matches('/'));
    match client.get(&url).timeout(SCRAPE_TIMEOUT).send().await {
        Ok(resp) => match resp.text().await {
            Ok(text) => parse_prometheus(&text),
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

/// Parse Prometheus text exposition format into raw Gauges.
/// No filtering, no unit conversion — every metric line becomes a Gauge.
fn parse_prometheus(text: &str) -> Vec<Gauge> {
    let mut gauges = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Split: metric_name{labels} value [timestamp]
        let (metric_part, value) = match parse_line(line) {
            Some(v) => v,
            None => continue,
        };

        // Extract metric name (before '{' or entire string)
        let name = match metric_part.find('{') {
            Some(i) => &metric_part[..i],
            None => metric_part,
        };

        // Extract labels from {key="val",key2="val2"}
        let labels = parse_labels(metric_part);

        gauges.push(Gauge {
            name: name.to_string(),
            value,
            labels,
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
        // key="value"
        let Some(eq) = rest.find('=') else { break };
        let key = &rest[..eq];
        let after_eq = &rest[eq + 1..];

        if !after_eq.starts_with('"') {
            break;
        }
        let val_start = 1; // skip opening quote
        let Some(val_end) = after_eq[val_start..].find('"') else {
            break;
        };
        let val = &after_eq[val_start..val_start + val_end];
        labels.push((key.to_string(), val.to_string()));

        // Skip past closing quote + optional comma
        let consumed = eq + 1 + val_start + val_end + 1; // key= + " + val + "
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

#[derive(Deserialize, Clone)]
struct OllamaPsModel {
    name: Option<String>,
    size_vram: Option<u64>,
    size: Option<u64>,
    expires_at: Option<String>,
}

/// Scrape Ollama /api/ps and return raw model data as Gauges.
/// No unit conversion — size_vram and size are forwarded as raw bytes.
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
    let mut gauges = Vec::with_capacity(models.len() * 3 + 1);

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
                labels: model_label.clone(),
            });
        }
        if let Some(ref expires) = model.expires_at {
            gauges.push(Gauge {
                name: "ollama_model_expires_at".into(),
                value: 1.0, // presence marker; actual timestamp in label
                labels: {
                    let mut l = model_label.clone();
                    l.push(("expires_at".into(), expires.clone()));
                    l
                },
            });
        }
    }

    gauges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_prometheus_passthrough() {
        let text = r#"
node_memory_MemTotal_bytes 6.7385810944e+10
node_memory_MemAvailable_bytes 5.0e+10
"#;
        let gauges = parse_prometheus(text);
        assert_eq!(gauges.len(), 2);
        let total = gauges.iter().find(|g| g.name == "node_memory_MemTotal_bytes").unwrap();
        assert!((total.value - 6.7385810944e10).abs() < 1.0);
    }

    #[test]
    fn parse_prometheus_preserves_labels() {
        let text = r#"
node_drm_gpu_busy_percent{card="card1"} 42
node_hwmon_temp_celsius{chip="0000:c4:00_0",sensor="temp1"} 55
"#;
        let gauges = parse_prometheus(text);
        assert_eq!(gauges.len(), 2);

        let busy = gauges.iter().find(|g| g.name == "node_drm_gpu_busy_percent").unwrap();
        assert!((busy.value - 42.0).abs() < 0.1);
        assert_eq!(busy.labels, vec![("card".into(), "card1".into())]);

        let temp = gauges.iter().find(|g| g.name == "node_hwmon_temp_celsius").unwrap();
        assert_eq!(temp.labels.len(), 2);
        assert_eq!(temp.labels[0], ("chip".into(), "0000:c4:00_0".into()));
        assert_eq!(temp.labels[1], ("sensor".into(), "temp1".into()));
    }

    #[test]
    fn parse_prometheus_cpu_lines() {
        let text = r#"
node_cpu_seconds_total{cpu="0",mode="idle"} 1000
node_cpu_seconds_total{cpu="0",mode="user"} 500
node_cpu_seconds_total{cpu="1",mode="idle"} 900
"#;
        let gauges = parse_prometheus(text);
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
