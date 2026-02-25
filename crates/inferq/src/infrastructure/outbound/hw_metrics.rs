/// Hardware metrics from inferq-agent (Valkey cache) and
/// live node-exporter fetch (Prometheus text format).
use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Valkey TTL for hardware metrics cache (seconds).
pub const HW_METRICS_TTL: i64 = 60;

pub fn hw_metrics_key(backend_id: Uuid) -> String {
    format!("inferq:hw:{backend_id}")
}

// ── Agent-based metrics (Phase 2) ──────────────────────────────────────────────

/// GPU + system RAM metrics as reported by inferq-agent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HwMetrics {
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
    pub gpu_util_pct: u8,
    pub power_w: f32,
    pub temp_c: f32,
    pub mem_used_mb: u32,
    pub mem_total_mb: u32,
    pub loaded_model_count: u8,
}

impl HwMetrics {
    /// Available VRAM in MiB (`vram_total_mb - vram_used_mb`).
    pub fn vram_free_mb(&self) -> i64 {
        self.vram_total_mb as i64 - self.vram_used_mb as i64
    }

    /// Returns `true` when the GPU temperature is at or above 85 °C.
    pub fn is_overheating(&self) -> bool {
        self.temp_c > 0.0 && self.temp_c >= 85.0
    }
}

// ── Valkey helpers ─────────────────────────────────────────────────────────────

/// Read cached hardware metrics for a backend from Valkey.
/// Returns `None` on cache miss, parse failure, or Valkey error.
pub async fn load_hw_metrics(
    pool: &fred::clients::RedisPool,
    backend_id: Uuid,
) -> Option<HwMetrics> {
    use fred::prelude::*;
    let key = hw_metrics_key(backend_id);
    let cached: Option<String> = pool.get(&key).await.unwrap_or(None);
    serde_json::from_str(&cached?).ok()
}

/// Write hardware metrics for a backend to Valkey (TTL = 60 s).
/// Errors are logged as warnings and ignored.
pub async fn store_hw_metrics(
    pool: &fred::clients::RedisPool,
    backend_id: Uuid,
    metrics: &HwMetrics,
) {
    use fred::prelude::*;
    let key = hw_metrics_key(backend_id);
    let Ok(json) = serde_json::to_string(metrics) else {
        return;
    };
    if let Err(e) = pool
        .set::<String, _, _>(key, json, Some(Expiration::EX(HW_METRICS_TTL)), None, false)
        .await
    {
        tracing::warn!(backend_id = %backend_id, "hw_metrics: failed to cache: {e}");
    }
}

// ── Node-exporter live fetch ───────────────────────────────────────────────────

/// Hardware metrics scraped live from a node-exporter `/metrics` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeMetrics {
    /// `false` when the node-exporter endpoint is unreachable.
    pub scrape_ok: bool,
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    /// Number of logical CPU cores detected.
    pub cpu_cores: u32,
    /// Per-GPU metrics (empty when no DRM/hwmon GPU data is available).
    pub gpus: Vec<GpuNodeMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuNodeMetrics {
    /// DRM card name, e.g. `"card0"`.
    pub card: String,
    /// GPU temperature in °C (hwmon).
    pub temp_c: Option<f64>,
    /// GPU power draw in Watts (hwmon).
    pub power_w: Option<f64>,
    /// VRAM used in MiB (DRM).
    pub vram_used_mb: Option<u64>,
    /// VRAM total in MiB (DRM).
    pub vram_total_mb: Option<u64>,
    /// GPU utilization 0–100 % (DRM).
    pub busy_pct: Option<f64>,
}

/// Fetch hardware metrics from a node-exporter endpoint.
///
/// Returns `NodeMetrics { scrape_ok: false, .. }` on network / parse errors
/// rather than propagating an error, so callers can return a graceful "not
/// reachable" response without treating connectivity issues as server errors.
pub async fn fetch_node_metrics(node_exporter_url: &str) -> Result<NodeMetrics> {
    let url = format!("{}/metrics", node_exporter_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let text = client.get(&url).send().await?.text().await?;
    Ok(parse_prometheus_metrics(&text))
}

// ── Prometheus text format parser ──────────────────────────────────────────────

/// Extract a label value from a metric line, e.g. `cpu="0"` → `"0"`.
fn get_label<'a>(metric_part: &'a str, key: &str) -> Option<&'a str> {
    let pattern = &format!("{key}=\"");
    let start = metric_part.find(pattern.as_str())? + pattern.len();
    let end = metric_part[start..].find('"')? + start;
    Some(&metric_part[start..end])
}

fn parse_prometheus_metrics(text: &str) -> NodeMetrics {
    use std::collections::{HashMap, HashSet};

    let mut mem_total_bytes: f64 = 0.0;
    let mut mem_available_bytes: f64 = 0.0;
    let mut cpu_set: HashSet<String> = HashSet::new();

    // DRM metrics keyed by card name ("card0", "card1", …)
    let mut drm_busy: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_used: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_total: HashMap<String, f64> = HashMap::new();

    // hwmon chip name lookup: chip_label → chip_name (e.g. "amdgpu").
    // node_hwmon_chip_names{chip="0000:00:08_1_0000:c4:00_0",chip_name="amdgpu"} 1
    // Needed because AMD APU chips use PCI address labels, not "amdgpu-pci-*" labels.
    let mut chip_name_map: HashMap<String, String> = HashMap::new();

    // hwmon metrics keyed by chip label; only amdgpu chips are collected.
    let mut hwmon_temp: HashMap<String, f64> = HashMap::new();
    let mut hwmon_power: HashMap<String, f64> = HashMap::new();
    let mut amdgpu_chips: HashSet<String> = HashSet::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        // Split "metric_name{labels}" from "value [timestamp]"
        let (metric_part, value_str) = {
            // Find the first space that is not inside a label block
            let split_at = if let Some(brace_end) = line.find('}') {
                line[brace_end..].find(' ').map(|i| brace_end + i)
            } else {
                line.find(' ')
            };
            let Some(idx) = split_at else { continue };
            (&line[..idx], line[idx + 1..].split_whitespace().next().unwrap_or(""))
        };

        let Ok(value) = value_str.parse::<f64>() else {
            continue;
        };

        let name_only = metric_part.split('{').next().unwrap_or(metric_part);

        match name_only {
            "node_memory_MemTotal_bytes" => mem_total_bytes = value,
            "node_memory_MemAvailable_bytes" => mem_available_bytes = value,

            "node_cpu_seconds_total" => {
                if let Some(cpu) = get_label(metric_part, "cpu") {
                    cpu_set.insert(cpu.to_string());
                }
            }

            // Build chip label → chip_name map for AMD APU hwmon lookup.
            // AMD APU chips use PCI-address chip labels (e.g. "0000:00:08_1_0000:c4:00_0")
            // while chip_name="amdgpu". This lets us match them even without "amdgpu" in
            // the chip label.
            "node_hwmon_chip_names" => {
                if let (Some(chip), Some(chip_name)) = (
                    get_label(metric_part, "chip"),
                    get_label(metric_part, "chip_name"),
                ) {
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
            "node_drm_memory_vram_total_bytes" => {
                if let Some(card) = get_label(metric_part, "card") {
                    drm_vram_total.insert(card.to_string(), value);
                }
            }

            "node_hwmon_temp_celsius" => {
                if let Some(chip) = get_label(metric_part, "chip") {
                    let is_amdgpu = chip.contains("amdgpu")
                        || chip_name_map.get(chip).map_or(false, |n| n == "amdgpu");
                    if is_amdgpu {
                        // Take the lowest-numbered sensor as the representative temp.
                        hwmon_temp.entry(chip.to_string()).or_insert(value);
                        amdgpu_chips.insert(chip.to_string());
                    }
                }
            }
            // node-exporter emits "node_hwmon_power_average_watt" (no trailing 's')
            // on some systems; accept both spellings.
            "node_hwmon_power_average_watts" | "node_hwmon_power_average_watt" => {
                if let Some(chip) = get_label(metric_part, "chip") {
                    let is_amdgpu = chip.contains("amdgpu")
                        || chip_name_map.get(chip).map_or(false, |n| n == "amdgpu");
                    if is_amdgpu {
                        hwmon_power.entry(chip.to_string()).or_insert(value);
                        amdgpu_chips.insert(chip.to_string());
                    }
                }
            }

            _ => {}
        }
    }

    // Build sorted GPU list.
    // Primary source: DRM metrics (most complete for AMD GPUs with --collector.drm).
    // Fallback: hwmon-only (if --collector.drm not enabled).
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

    let gpus: Vec<GpuNodeMetrics> = if !drm_cards.is_empty() {
        drm_cards
            .iter()
            .map(|card| {
                // Correlate DRM card index with amdgpu hwmon chip by position.
                let card_idx: usize =
                    card.trim_start_matches("card").parse().unwrap_or(0);
                let chip = chips_sorted.get(card_idx);

                GpuNodeMetrics {
                    card: card.clone(),
                    temp_c: chip.and_then(|c| hwmon_temp.get(c)).copied(),
                    power_w: chip.and_then(|c| hwmon_power.get(c)).copied(),
                    vram_used_mb: drm_vram_used
                        .get(card)
                        .map(|b| (*b / 1_048_576.0) as u64),
                    vram_total_mb: drm_vram_total
                        .get(card)
                        .map(|b| (*b / 1_048_576.0) as u64),
                    busy_pct: drm_busy.get(card).copied(),
                }
            })
            .collect()
    } else if !chips_sorted.is_empty() {
        // hwmon-only fallback (no DRM collector)
        chips_sorted
            .iter()
            .enumerate()
            .map(|(i, chip)| GpuNodeMetrics {
                card: format!("card{i}"),
                temp_c: hwmon_temp.get(chip).copied(),
                power_w: hwmon_power.get(chip).copied(),
                vram_used_mb: None,
                vram_total_mb: None,
                busy_pct: None,
            })
            .collect()
    } else {
        vec![]
    };

    NodeMetrics {
        scrape_ok: true,
        mem_total_mb: (mem_total_bytes / 1_048_576.0) as u64,
        mem_available_mb: (mem_available_bytes / 1_048_576.0) as u64,
        cpu_cores: cpu_set.len() as u32,
        gpus,
    }
}
