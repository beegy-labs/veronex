/// Scrapes node-exporter (Prometheus text) and Ollama API, returning a flat
/// list of gauge metrics ready for OTLP push.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::Deserialize;

/// A single gauge metric with name, value, and optional extra labels.
pub struct Gauge {
    pub name: &'static str,
    pub value: f64,
    pub labels: Vec<(&'static str, String)>,
}

// ── Node-exporter ────────────────────────────────────────────────────────────

/// Scrape node-exporter + Ollama and return combined gauge list.
pub async fn scrape(
    client: &reqwest::Client,
    node_exporter_url: &str,
    ollama_url: &str,
) -> Vec<Gauge> {
    let mut gauges = Vec::new();

    // node-exporter
    match scrape_node_exporter(client, node_exporter_url).await {
        Ok(g) => gauges.extend(g),
        Err(e) => tracing::warn!("node-exporter scrape failed: {e}"),
    }

    // Ollama
    match scrape_ollama(client, ollama_url).await {
        Ok(g) => gauges.extend(g),
        Err(e) => tracing::debug!("ollama scrape skipped: {e}"),
    }

    gauges
}

async fn scrape_node_exporter(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<Vec<Gauge>> {
    let url = format!("{}/metrics", base_url.trim_end_matches('/'));
    let text = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .text()
        .await?;
    Ok(parse_node_exporter(&text))
}

/// Extract label value from Prometheus metric line.
fn get_label<'a>(metric_part: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("{key}=\"");
    let start = metric_part.find(pattern.as_str())? + pattern.len();
    let end = metric_part[start..].find('"')? + start;
    Some(&metric_part[start..end])
}

fn parse_node_exporter(text: &str) -> Vec<Gauge> {
    let mut gauges = Vec::new();
    let mut mem_total: f64 = 0.0;
    let mut mem_available: f64 = 0.0;

    // CPU tracking
    let mut cpu_set: HashSet<String> = HashSet::new();
    let mut _cpu_idle_sum: f64 = 0.0;
    let mut _cpu_total_sum: f64 = 0.0;

    // GPU (DRM + hwmon)
    let mut drm_busy: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_used: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_total: HashMap<String, f64> = HashMap::new();
    let mut chip_name_map: HashMap<String, String> = HashMap::new();
    let mut hwmon_temp: HashMap<String, f64> = HashMap::new();
    let mut hwmon_power: HashMap<String, f64> = HashMap::new();
    let mut amdgpu_chips: HashSet<String> = HashSet::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        let (metric_part, value_str) = {
            let split_at = if let Some(brace_end) = line.find('}') {
                line[brace_end..].find(' ').map(|i| brace_end + i)
            } else {
                line.find(' ')
            };
            let Some(idx) = split_at else { continue };
            (&line[..idx], line[idx + 1..].split_whitespace().next().unwrap_or(""))
        };

        let Ok(value) = value_str.parse::<f64>() else { continue };
        let name = metric_part.split('{').next().unwrap_or(metric_part);

        match name {
            "node_memory_MemTotal_bytes" => mem_total = value,
            "node_memory_MemAvailable_bytes" => mem_available = value,
            "node_cpu_seconds_total" => {
                if let Some(cpu) = get_label(metric_part, "cpu") {
                    cpu_set.insert(cpu.to_string());
                }
                _cpu_total_sum += value;
                if get_label(metric_part, "mode") == Some("idle") {
                    _cpu_idle_sum += value;
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
            "node_hwmon_temp_celsius" => {
                if let Some(chip) = get_label(metric_part, "chip") {
                    let is_amdgpu = chip.contains("amdgpu")
                        || chip_name_map.get(chip).is_some_and(|n| n == "amdgpu");
                    if is_amdgpu {
                        hwmon_temp.entry(chip.to_string()).or_insert(value);
                        amdgpu_chips.insert(chip.to_string());
                    }
                }
            }
            "node_hwmon_power_average_watts" | "node_hwmon_power_average_watt" => {
                if let Some(chip) = get_label(metric_part, "chip") {
                    let is_amdgpu = chip.contains("amdgpu")
                        || chip_name_map.get(chip).is_some_and(|n| n == "amdgpu");
                    if is_amdgpu {
                        hwmon_power.entry(chip.to_string()).or_insert(value);
                        amdgpu_chips.insert(chip.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // System memory
    let mem_total_mb = mem_total / 1_048_576.0;
    let mem_used_mb = (mem_total - mem_available) / 1_048_576.0;
    gauges.push(Gauge { name: "system.memory.total", value: mem_total_mb, labels: vec![] });
    gauges.push(Gauge { name: "system.memory.used", value: mem_used_mb, labels: vec![] });

    // CPU
    gauges.push(Gauge {
        name: "system.cpu.count",
        value: cpu_set.len() as f64,
        labels: vec![],
    });

    // Build GPU metrics
    let mut chips_sorted: Vec<String> = amdgpu_chips.into_iter().collect();
    chips_sorted.sort();

    let mut drm_cards: Vec<String> = drm_busy
        .keys()
        .chain(drm_vram_used.keys())
        .chain(drm_vram_total.keys())
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    drm_cards.sort();

    let gpu_sources: Vec<(String, Option<&String>)> = if !drm_cards.is_empty() {
        drm_cards
            .iter()
            .enumerate()
            .map(|(i, card)| (card.clone(), chips_sorted.get(i)))
            .collect()
    } else if !chips_sorted.is_empty() {
        chips_sorted
            .iter()
            .enumerate()
            .map(|(i, chip)| (format!("card{i}"), Some(chip)))
            .collect()
    } else {
        vec![]
    };

    for (idx, (card, chip)) in gpu_sources.iter().enumerate() {
        let gpu_label = vec![("gpu", idx.to_string())];

        if let Some(v) = drm_vram_used.get(card) {
            gauges.push(Gauge {
                name: "gpu.vram.used",
                value: v / 1_048_576.0,
                labels: gpu_label.clone(),
            });
        }
        if let Some(v) = drm_vram_total.get(card) {
            gauges.push(Gauge {
                name: "gpu.vram.total",
                value: v / 1_048_576.0,
                labels: gpu_label.clone(),
            });
        }
        if let Some(v) = drm_busy.get(card) {
            gauges.push(Gauge {
                name: "gpu.utilization",
                value: *v,
                labels: gpu_label.clone(),
            });
        }
        if let Some(chip) = chip {
            if let Some(v) = hwmon_temp.get(*chip) {
                gauges.push(Gauge {
                    name: "gpu.temperature",
                    value: *v,
                    labels: gpu_label.clone(),
                });
            }
            if let Some(v) = hwmon_power.get(*chip) {
                gauges.push(Gauge {
                    name: "gpu.power",
                    value: *v,
                    labels: gpu_label.clone(),
                });
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

async fn scrape_ollama(
    client: &reqwest::Client,
    ollama_url: &str,
) -> anyhow::Result<Vec<Gauge>> {
    let url = format!("{}/api/ps", ollama_url.trim_end_matches('/'));
    let resp: OllamaPsResponse = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .json()
        .await?;

    let models = resp.models.unwrap_or_default();
    let mut gauges = Vec::new();

    gauges.push(Gauge {
        name: "ollama.loaded_models",
        value: models.len() as f64,
        labels: vec![],
    });

    for model in &models {
        let model_name = model.name.as_deref().unwrap_or("unknown");
        let vram_mb = model.size_vram.unwrap_or(0) as f64 / 1_048_576.0;
        gauges.push(Gauge {
            name: "ollama.model.vram",
            value: vram_mb,
            labels: vec![("model", model_name.to_string())],
        });
    }

    Ok(gauges)
}
