/// Scrapes node-exporter (Prometheus text) and Ollama API independently,
/// returning flat gauge lists ready for OTLP push.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::Deserialize;

const SCRAPE_TIMEOUT: Duration = Duration::from_secs(5);

/// A single gauge metric with name, value, and optional extra labels.
pub struct Gauge {
    pub name: &'static str,
    pub value: f64,
    pub labels: Vec<(&'static str, String)>,
}

// ── Node-exporter ────────────────────────────────────────────────────────────

/// Scrape node-exporter and return hardware gauge list.
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

/// Extract label value from Prometheus metric line.
fn get_label<'a>(metric_part: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("{key}=\"");
    let start = metric_part.find(pattern.as_str())? + pattern.len();
    let end = metric_part[start..].find('"')? + start;
    Some(&metric_part[start..end])
}

/// Parse a Prometheus metric line into (metric_part, value).
fn parse_line(line: &str) -> Option<(&str, f64)> {
    let line = line.trim();
    if line.starts_with('#') || line.is_empty() {
        return None;
    }
    let split_at = if let Some(brace_end) = line.find('}') {
        line[brace_end..].find(' ').map(|i| brace_end + i)
    } else {
        line.find(' ')
    }?;
    let metric_part = &line[..split_at];
    let value: f64 = line[split_at + 1..].split_whitespace().next()?.parse().ok()?;
    Some((metric_part, value))
}

fn parse_node_exporter(text: &str) -> Vec<Gauge> {
    let mut gauges = Vec::new();
    let mut mem_total: f64 = 0.0;
    let mut mem_available: f64 = 0.0;
    let mut cpu_set: HashSet<String> = HashSet::new();

    let mut drm_busy: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_used: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_total: HashMap<String, f64> = HashMap::new();
    let mut chip_name_map: HashMap<String, String> = HashMap::new();
    let mut hwmon_temp: HashMap<String, f64> = HashMap::new();
    let mut hwmon_power: HashMap<String, f64> = HashMap::new();
    let mut amdgpu_chips: HashSet<String> = HashSet::new();

    for line in text.lines() {
        let Some((metric_part, value)) = parse_line(line) else {
            continue;
        };
        let name = metric_part.split('{').next().unwrap_or(metric_part);

        match name {
            "node_memory_MemTotal_bytes" => mem_total = value,
            "node_memory_MemAvailable_bytes" => mem_available = value,
            "node_cpu_seconds_total" => {
                if let Some(cpu) = get_label(metric_part, "cpu") {
                    cpu_set.insert(cpu.to_string());
                }
            }
            "node_hwmon_chip_names" => {
                if let (Some(chip), Some(chip_name)) =
                    (get_label(metric_part, "chip"), get_label(metric_part, "chip_name"))
                {
                    chip_name_map.insert(chip.to_string(), chip_name.to_string());
                }
            }
            "node_drm_gpu_busy_percent" => {
                if let Some(card) = get_label(metric_part, "card") {
                    drm_busy.entry(card.to_string()).or_insert(value);
                }
            }
            "node_drm_memory_vram_used_bytes" => {
                if let Some(card) = get_label(metric_part, "card") {
                    drm_vram_used.insert(card.to_string(), value);
                }
            }
            "node_drm_memory_vram_size_bytes" => {
                if let Some(card) = get_label(metric_part, "card") {
                    drm_vram_total.entry(card.to_string()).or_insert(value);
                }
            }
            "node_drm_memory_vram_total_bytes" => {
                if let Some(card) = get_label(metric_part, "card") {
                    drm_vram_total.insert(card.to_string(), value);
                }
            }
            "node_hwmon_temp_celsius" | "node_hwmon_power_average_watts"
            | "node_hwmon_power_average_watt" => {
                if let Some(chip) = get_label(metric_part, "chip") {
                    let is_amdgpu = chip.contains("amdgpu")
                        || chip_name_map.get(chip).is_some_and(|n| n == "amdgpu");
                    if is_amdgpu {
                        amdgpu_chips.insert(chip.to_string());
                        if name == "node_hwmon_temp_celsius" {
                            hwmon_temp.entry(chip.to_string()).or_insert(value);
                        } else {
                            hwmon_power.entry(chip.to_string()).or_insert(value);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    gauges.push(Gauge { name: "system.memory.total", value: mem_total / 1_048_576.0, labels: vec![] });
    gauges.push(Gauge { name: "system.memory.used", value: (mem_total - mem_available) / 1_048_576.0, labels: vec![] });
    gauges.push(Gauge { name: "system.cpu.count", value: cpu_set.len() as f64, labels: vec![] });

    let mut chips_sorted: Vec<String> = amdgpu_chips.into_iter().collect();
    chips_sorted.sort();

    let mut drm_cards: Vec<String> = drm_busy.keys()
        .chain(drm_vram_used.keys())
        .chain(drm_vram_total.keys())
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    drm_cards.sort();

    let gpu_sources: Vec<(String, Option<&String>)> = if !drm_cards.is_empty() {
        drm_cards.iter().enumerate().map(|(i, card)| (card.clone(), chips_sorted.get(i))).collect()
    } else if !chips_sorted.is_empty() {
        chips_sorted.iter().enumerate().map(|(i, chip)| (format!("card{i}"), Some(chip))).collect()
    } else {
        vec![]
    };

    for (idx, (card, chip)) in gpu_sources.iter().enumerate() {
        let gpu_label = vec![("gpu", idx.to_string())];
        if let Some(v) = drm_vram_used.get(card) {
            gauges.push(Gauge { name: "gpu.vram.used", value: v / 1_048_576.0, labels: gpu_label.clone() });
        }
        if let Some(v) = drm_vram_total.get(card) {
            gauges.push(Gauge { name: "gpu.vram.total", value: v / 1_048_576.0, labels: gpu_label.clone() });
        }
        if let Some(v) = drm_busy.get(card) {
            gauges.push(Gauge { name: "gpu.utilization", value: *v, labels: gpu_label.clone() });
        }
        if let Some(chip) = chip {
            if let Some(v) = hwmon_temp.get(*chip) {
                gauges.push(Gauge { name: "gpu.temperature", value: *v, labels: gpu_label.clone() });
            }
            if let Some(v) = hwmon_power.get(*chip) {
                gauges.push(Gauge { name: "gpu.power", value: *v, labels: gpu_label.clone() });
            }
        }
    }

    gauges
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
}

/// Scrape Ollama /api/ps and return model gauge list.
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
    let mut gauges = vec![Gauge {
        name: "ollama.loaded_models",
        value: models.len() as f64,
        labels: vec![],
    }];

    for model in &models {
        gauges.push(Gauge {
            name: "ollama.model.vram",
            value: model.size_vram.unwrap_or(0) as f64 / 1_048_576.0,
            labels: vec![("model", model.name.as_deref().unwrap_or("unknown").to_string())],
        });
    }

    gauges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_memory_metrics() {
        let text = r#"
node_memory_MemTotal_bytes 6.7385810944e+10
node_memory_MemAvailable_bytes 5.0e+10
"#;
        let gauges = parse_node_exporter(text);
        let total = gauges.iter().find(|g| g.name == "system.memory.total").unwrap();
        assert!((total.value - 64264.0).abs() < 1.0);
        let used = gauges.iter().find(|g| g.name == "system.memory.used").unwrap();
        assert!((used.value - 16580.0).abs() < 1.0);
    }

    #[test]
    fn parse_gpu_drm_metrics() {
        let text = r#"
node_drm_gpu_busy_percent{card="card1"} 42
node_drm_memory_vram_used_bytes{card="card1"} 4294967296
node_drm_memory_vram_total_bytes{card="card1"} 103079215104
node_hwmon_chip_names{chip="0000:c4:00_0",chip_name="amdgpu"} 1
node_hwmon_temp_celsius{chip="0000:c4:00_0",sensor="temp1"} 55
node_hwmon_power_average_watts{chip="0000:c4:00_0"} 30
"#;
        let gauges = parse_node_exporter(text);
        let util = gauges.iter().find(|g| g.name == "gpu.utilization").unwrap();
        assert!((util.value - 42.0).abs() < 0.1);
        let vram = gauges.iter().find(|g| g.name == "gpu.vram.used").unwrap();
        assert!((vram.value - 4096.0).abs() < 1.0);
        let temp = gauges.iter().find(|g| g.name == "gpu.temperature").unwrap();
        assert!((temp.value - 55.0).abs() < 0.1);
    }

    #[test]
    fn parse_cpu_count() {
        let text = r#"
node_cpu_seconds_total{cpu="0",mode="idle"} 1000
node_cpu_seconds_total{cpu="0",mode="user"} 500
node_cpu_seconds_total{cpu="1",mode="idle"} 900
node_cpu_seconds_total{cpu="1",mode="user"} 600
"#;
        let gauges = parse_node_exporter(text);
        let count = gauges.iter().find(|g| g.name == "system.cpu.count").unwrap();
        assert!((count.value - 2.0).abs() < 0.1);
    }

    #[test]
    fn get_label_extracts_value() {
        assert_eq!(get_label(r#"foo{cpu="3",mode="idle"}"#, "cpu"), Some("3"));
        assert_eq!(get_label(r#"foo{cpu="3",mode="idle"}"#, "mode"), Some("idle"));
        assert_eq!(get_label(r#"foo{cpu="3"}"#, "missing"), None);
    }
}
