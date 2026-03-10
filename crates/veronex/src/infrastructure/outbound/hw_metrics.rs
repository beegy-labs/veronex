/// Hardware metrics from node-exporter (Valkey cache) and
/// live node-exporter fetch (Prometheus text format).
use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Valkey TTL for hardware metrics cache (seconds).
pub const HW_METRICS_TTL: i64 = 60;

pub fn hw_metrics_key(provider_id: Uuid) -> String {
    super::valkey_keys::hw_metrics(provider_id)
}

// ── Hardware metrics (from node-exporter) ─────────────────────────────────────

/// GPU + system RAM metrics collected from node-exporter.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HwMetrics {
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
    pub gpu_util_pct: u8,
    pub power_w: f32,
    /// GPU edge temperature (°C). Kept for backward compat + logging.
    pub temp_c: f32,
    /// GPU junction/hotspot temperature (°C). Primary throttle input.
    #[serde(default)]
    pub temp_junction_c: f32,
    /// GPU memory temperature (°C). VRAM thermal protection.
    #[serde(default)]
    pub temp_mem_c: f32,
    pub mem_used_mb: u32,
    pub mem_total_mb: u32,
    /// GPU vendor from sysfs: "amd", "nvidia", or empty.
    #[serde(default)]
    pub gpu_vendor: String,
}

impl HwMetrics {
    /// Available VRAM in MiB (`vram_total_mb - vram_used_mb`).
    pub fn vram_free_mb(&self) -> i64 {
        self.vram_total_mb as i64 - self.vram_used_mb as i64
    }

    /// Worst-case temperature across all GPU sensors.
    /// Used for thermal throttle decisions — junction is typically the hottest.
    pub fn max_temp_c(&self) -> f32 {
        self.temp_c.max(self.temp_junction_c).max(self.temp_mem_c)
    }

    /// Returns `true` when any GPU sensor is at or above 85 °C.
    pub fn is_overheating(&self) -> bool {
        self.max_temp_c() >= 85.0
    }
}

// ── Valkey helpers ─────────────────────────────────────────────────────────────

/// Read cached hardware metrics for a provider from Valkey.
/// Returns `None` on cache miss, parse failure, or Valkey error.
pub async fn load_hw_metrics(
    pool: &fred::clients::Pool,
    provider_id: Uuid,
) -> Option<HwMetrics> {
    use fred::prelude::*;
    let key = hw_metrics_key(provider_id);
    let cached: Option<String> = pool.get(&key).await.unwrap_or(None);
    serde_json::from_str(&cached?).ok()
}

/// Write hardware metrics for a provider to Valkey (TTL = 60 s).
/// Errors are logged as warnings and ignored.
pub async fn store_hw_metrics(
    pool: &fred::clients::Pool,
    provider_id: Uuid,
    metrics: &HwMetrics,
) {
    use fred::prelude::*;
    let key = hw_metrics_key(provider_id);
    let Ok(json) = serde_json::to_string(metrics) else {
        return;
    };
    if let Err(e) = pool
        .set::<String, _, _>(key, json, Some(Expiration::EX(HW_METRICS_TTL)), None, false)
        .await
    {
        tracing::warn!(provider_id = %provider_id, "hw_metrics: failed to cache: {e}");
    }
}

// ── Node-exporter live fetch ───────────────────────────────────────────────────

/// Raw CPU counter snapshot used for delta-based usage calculation.
/// Two consecutive snapshots are needed to compute instantaneous CPU %.
#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    /// Sum of idle seconds across all CPUs.
    pub idle: f64,
    /// Sum of all mode seconds across all CPUs.
    pub total: f64,
}

/// Hardware metrics scraped live from a node-exporter `/metrics` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeMetrics {
    /// `false` when the node-exporter endpoint is unreachable.
    pub scrape_ok: bool,
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    /// Number of logical CPUs (hardware threads) — from node_cpu_seconds_total.
    pub cpu_logical: u32,
    /// Number of physical cores — from node_cpu_info (core × package pairs).
    /// `None` when node_cpu_info is not exported by node-exporter.
    pub cpu_physical: Option<u32>,
    /// Instantaneous CPU usage 0–100 %. `None` on the first scrape (no delta yet).
    pub cpu_usage_pct: Option<f64>,
    /// Per-GPU metrics (empty when no DRM/hwmon GPU data is available).
    pub gpus: Vec<GpuNodeMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuNodeMetrics {
    /// DRM card name, e.g. `"card0"`.
    pub card: String,
    /// GPU edge temperature in °C (hwmon sensor=temp1).
    pub temp_c: Option<f64>,
    /// GPU junction/hotspot temperature in °C (hwmon sensor=temp2).
    /// Highest point on die — primary throttle trigger.
    pub temp_junction_c: Option<f64>,
    /// GPU memory (HBM/GDDR) temperature in °C (hwmon sensor=temp3).
    /// VRAM overheating causes silent data corruption in LLM inference.
    pub temp_mem_c: Option<f64>,
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
pub async fn fetch_node_metrics(
    node_exporter_url: &str,
    prev_snapshot: Option<&CpuSnapshot>,
) -> Result<(NodeMetrics, CpuSnapshot)> {
    let url = format!("{}/metrics", node_exporter_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(crate::domain::constants::NODE_EXPORTER_TIMEOUT)
        .build()?;

    let text = client.get(&url).send().await?.text().await?;
    let (mut metrics, snapshot) = parse_prometheus_metrics(&text);

    if let Some(prev) = prev_snapshot {
        let delta_idle = (snapshot.idle - prev.idle).max(0.0);
        let delta_total = (snapshot.total - prev.total).max(0.0);
        if delta_total > 0.0 {
            metrics.cpu_usage_pct =
                Some(((1.0 - delta_idle / delta_total) * 100.0).clamp(0.0, 100.0));
        }
    }

    Ok((metrics, snapshot))
}

// ── Prometheus text format parser ──────────────────────────────────────────────

/// Extract a label value from a metric line, e.g. `cpu="0"` → `"0"`.
fn get_label<'a>(metric_part: &'a str, key: &str) -> Option<&'a str> {
    let pattern = &format!("{key}=\"");
    let start = metric_part.find(pattern.as_str())? + pattern.len();
    let end = metric_part[start..].find('"')? + start;
    Some(&metric_part[start..end])
}

fn parse_prometheus_metrics(text: &str) -> (NodeMetrics, CpuSnapshot) {
    use std::collections::{HashMap, HashSet};

    let mut mem_total_bytes: f64 = 0.0;
    let mut mem_available_bytes: f64 = 0.0;
    // Logical CPUs from node_cpu_seconds_total (always available).
    let mut cpu_set: HashSet<String> = HashSet::new();
    // Physical core count from node_cpu_info (core:package pairs).
    let mut cpu_physical_set: HashSet<String> = HashSet::new();
    // Tracks whether node_cpu_info was present at all.
    let mut cpu_info_seen = false;
    // CPU usage delta tracking: sum idle and total seconds across all CPUs.
    let mut cpu_idle_sum: f64 = 0.0;
    let mut cpu_total_sum: f64 = 0.0;

    // DRM metrics keyed by card name ("card0", "card1", …)
    let mut drm_busy: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_used: HashMap<String, f64> = HashMap::new();
    let mut drm_vram_total: HashMap<String, f64> = HashMap::new();

    // hwmon chip name lookup: chip_label → chip_name (e.g. "amdgpu").
    // node_hwmon_chip_names{chip="0000:00:08_1_0000:c4:00_0",chip_name="amdgpu"} 1
    // Needed because AMD APU chips use PCI address labels, not "amdgpu-pci-*" labels.
    let mut chip_name_map: HashMap<String, String> = HashMap::new();

    // hwmon metrics keyed by chip label; only amdgpu chips are collected.
    // Keyed as "chip:sensor" for per-sensor lookup (temp1=edge, temp2=junction, temp3=memory).
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
                cpu_total_sum += value;
                if get_label(metric_part, "mode") == Some("idle") {
                    cpu_idle_sum += value;
                }
            }

            // node_cpu_info{cpu="0",core="0",package="0",...} 1
            // Available when node-exporter is run with --collector.cpu.info.
            // `core:package` pairs = physical cores; `cpu` = logical threads.
            "node_cpu_info" => {
                cpu_info_seen = true;
                if let (Some(core), Some(package)) = (
                    get_label(metric_part, "core"),
                    get_label(metric_part, "package"),
                ) {
                    cpu_physical_set.insert(format!("{core}:{package}"));
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
            // node-exporter ≥0.16 uses "vram_size_bytes"; older versions "vram_total_bytes".
            // Accept both; "total" overwrites "size" if both appear.
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
                        let sensor = get_label(metric_part, "sensor").unwrap_or("temp1");
                        hwmon_temp.insert(format!("{chip}:{sensor}"), value);
                        amdgpu_chips.insert(chip.to_string());
                    }
                }
            }
            // node-exporter emits "node_hwmon_power_average_watt" (no trailing 's')
            // on some systems; accept both spellings.
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
            .enumerate()
            .map(|(card_position, card)| {
                // Correlate by sorted position, not by the number in the card name.
                // AMD APUs (e.g. AI 395+) expose as "card1" while there is only one
                // amdgpu hwmon chip at chips_sorted[0]; using the parsed card_idx
                // would give chips_sorted[1] = None and lose temp/power data.
                let chip = chips_sorted.get(card_position);

                GpuNodeMetrics {
                    card: card.clone(),
                    temp_c: chip.and_then(|c| hwmon_temp.get(&format!("{c}:temp1"))).copied(),
                    temp_junction_c: chip.and_then(|c| hwmon_temp.get(&format!("{c}:temp2"))).copied(),
                    temp_mem_c: chip.and_then(|c| hwmon_temp.get(&format!("{c}:temp3"))).copied(),
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
                temp_c: hwmon_temp.get(&format!("{chip}:temp1")).copied(),
                temp_junction_c: hwmon_temp.get(&format!("{chip}:temp2")).copied(),
                temp_mem_c: hwmon_temp.get(&format!("{chip}:temp3")).copied(),
                power_w: hwmon_power.get(chip).copied(),
                vram_used_mb: None,
                vram_total_mb: None,
                busy_pct: None,
            })
            .collect()
    } else {
        vec![]
    };

    let metrics = NodeMetrics {
        scrape_ok: true,
        mem_total_mb: (mem_total_bytes / 1_048_576.0) as u64,
        mem_available_mb: (mem_available_bytes / 1_048_576.0) as u64,
        cpu_logical: cpu_set.len() as u32,
        cpu_physical: if cpu_info_seen && !cpu_physical_set.is_empty() {
            Some(cpu_physical_set.len() as u32)
        } else {
            None
        },
        cpu_usage_pct: None, // filled in by caller after delta computation
        gpus,
    };

    let snapshot = CpuSnapshot {
        idle: cpu_idle_sum,
        total: cpu_total_sum,
    };

    (metrics, snapshot)
}
