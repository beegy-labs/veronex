/// Hardware metrics from node-exporter (Valkey cache) and
/// live node-exporter fetch (Prometheus text format).
use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::constants::{HW_METRICS_TTL, NODE_METRICS_TTL};

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
    /// System RAM available in MiB (from node-exporter).
    /// For APU unified memory, this replaces DRM VRAM as the capacity source.
    #[serde(default)]
    pub mem_available_mb: u32,
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
    let key = super::valkey_keys::hw_metrics(provider_id);
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
    let key = super::valkey_keys::hw_metrics(provider_id);
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

/// Read cached NodeMetrics for a GPU server from Valkey.
pub async fn load_node_metrics(
    pool: &fred::clients::Pool,
    server_id: Uuid,
) -> Option<NodeMetrics> {
    use fred::prelude::*;
    let key = super::valkey_keys::server_node_metrics(server_id);
    let cached: Option<String> = pool.get(&key).await.unwrap_or(None);
    serde_json::from_str(&cached?).ok()
}

/// Write NodeMetrics for a GPU server to Valkey (TTL = 60 s).
pub async fn store_node_metrics(
    pool: &fred::clients::Pool,
    server_id: Uuid,
    metrics: &NodeMetrics,
) {
    use fred::prelude::*;
    let key = super::valkey_keys::server_node_metrics(server_id);
    let Ok(json) = serde_json::to_string(metrics) else {
        return;
    };
    if let Err(e) = pool
        .set::<String, _, _>(key, json, Some(Expiration::EX(NODE_METRICS_TTL)), None, false)
        .await
    {
        tracing::warn!(server_id = %server_id, "node_metrics: failed to cache: {e}");
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
    client: Option<&reqwest::Client>,
) -> Result<(NodeMetrics, CpuSnapshot)> {
    let url = format!("{}/metrics", node_exporter_url.trim_end_matches('/'));

    let owned;
    let client = match client {
        Some(c) => c,
        None => {
            owned = reqwest::Client::builder()
                .timeout(crate::domain::constants::NODE_EXPORTER_TIMEOUT)
                .build()?;
            &owned
        }
    };

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

/// Known GPU chip_names from hwmon sysfs:
///   - "amdgpu"  : AMD dGPU / APU (full hwmon + DRM support)
///   - "nouveau" : NVIDIA open-source driver (hwmon temp only; proprietary driver needs dcgm)
///   - "i915"    : Intel integrated/discrete GPU (older kernel driver)
///   - "xe"      : Intel discrete GPU (newer kernel driver, Arc series)
const GPU_CHIP_NAMES: &[&str] = &["amdgpu", "nouveau", "i915", "xe"];

/// Check if a hwmon chip label belongs to a GPU.
/// Matches by chip label substring (e.g. "amdgpu-pci-0300") or chip_name_map lookup.
fn is_gpu_chip(chip: &str, chip_name_map: &std::collections::HashMap<String, String>) -> bool {
    GPU_CHIP_NAMES.iter().any(|name| chip.contains(name))
        || chip_name_map
            .get(chip)
            .is_some_and(|n| GPU_CHIP_NAMES.contains(&n.as_str()))
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

    // hwmon chip name lookup: chip_label → chip_name (e.g. "amdgpu", "nouveau").
    // node_hwmon_chip_names{chip="0000:00:08_1_0000:c4:00_0",chip_name="amdgpu"} 1
    // Needed because AMD APU chips use PCI address labels, not "amdgpu-pci-*" labels.
    let mut chip_name_map: HashMap<String, String> = HashMap::new();

    // hwmon metrics keyed by chip label.
    // Keyed as "chip:sensor" for per-sensor lookup (temp1=edge, temp2=junction, temp3=memory).
    // Supported GPU chip_names:
    //   AMD:    "amdgpu"  — full hwmon (temp/power) + DRM (vram/busy)
    //   NVIDIA: "nouveau" — hwmon temp only (open-source driver; proprietary needs dcgm-exporter)
    //   Intel:  "i915", "xe" — hwmon temp (limited)
    let mut hwmon_temp: HashMap<String, f64> = HashMap::new();
    let mut hwmon_power: HashMap<String, f64> = HashMap::new();
    let mut gpu_chips: HashSet<String> = HashSet::new();

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
            // Linux: node_memory_MemTotal_bytes / node_memory_MemAvailable_bytes
            // macOS: node_memory_total_bytes / node_memory_free_bytes
            "node_memory_MemTotal_bytes" | "node_memory_total_bytes" => mem_total_bytes = value,
            "node_memory_MemAvailable_bytes" | "node_memory_free_bytes" => {
                mem_available_bytes = value
            }

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

            // Build chip label → chip_name map for GPU hwmon lookup.
            // AMD APU chips use PCI-address chip labels (e.g. "0000:00:08_1_0000:c4:00_0")
            // while chip_name="amdgpu". This lets us match them even without "amdgpu" in
            // the chip label. Same pattern applies to nouveau/i915/xe.
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
                    if is_gpu_chip(chip, &chip_name_map) {
                        let sensor = get_label(metric_part, "sensor").unwrap_or("temp1");
                        hwmon_temp.insert(format!("{chip}:{sensor}"), value);
                        gpu_chips.insert(chip.to_string());
                    }
                }
            }
            // node-exporter emits "node_hwmon_power_average_watt" (no trailing 's')
            // on some systems; accept both spellings.
            "node_hwmon_power_average_watts" | "node_hwmon_power_average_watt" => {
                if let Some(chip) = get_label(metric_part, "chip") {
                    if is_gpu_chip(chip, &chip_name_map) {
                        hwmon_power.entry(chip.to_string()).or_insert(value);
                        gpu_chips.insert(chip.to_string());
                    }
                }
            }

            _ => {}
        }
    }

    // Build sorted GPU list.
    // Primary source: DRM metrics (AMD GPUs with --collector.drm).
    // Fallback: hwmon-only (NVIDIA nouveau, Intel i915/xe, or AMD without DRM collector).
    let mut chips_sorted: Vec<String> = gpu_chips.into_iter().collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Memory metrics ────────────────────────────────────────────────────

    #[test]
    fn mem_total_bytes_converted_to_mb() {
        let text = "node_memory_MemTotal_bytes 8589934592\n\
                    node_memory_MemAvailable_bytes 4294967296\n";
        let (metrics, _) = parse_prometheus_metrics(text);
        // 8589934592 / 1048576 = 8192 MiB
        assert_eq!(metrics.mem_total_mb, 8192);
        // 4294967296 / 1048576 = 4096 MiB
        assert_eq!(metrics.mem_available_mb, 4096);
    }

    #[test]
    fn mem_available_present_but_total_missing() {
        let text = "node_memory_MemAvailable_bytes 2147483648\n";
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.mem_total_mb, 0);
        assert_eq!(metrics.mem_available_mb, 2048);
    }

    // ── CPU logical / physical core detection ────────────────────────────

    #[test]
    fn cpu_logical_count_from_cpu_seconds_total() {
        // Four CPUs (0-3), two modes each — logical count should be 4
        let text = r#"node_cpu_seconds_total{cpu="0",mode="idle"} 1000.0
node_cpu_seconds_total{cpu="0",mode="user"} 200.0
node_cpu_seconds_total{cpu="1",mode="idle"} 1001.0
node_cpu_seconds_total{cpu="1",mode="user"} 201.0
node_cpu_seconds_total{cpu="2",mode="idle"} 1002.0
node_cpu_seconds_total{cpu="3",mode="idle"} 1003.0
"#;
        let (metrics, snapshot) = parse_prometheus_metrics(text);
        assert_eq!(metrics.cpu_logical, 4);
        assert_eq!(metrics.cpu_physical, None); // node_cpu_info not present
        // idle sum: 1000 + 1001 + 1002 + 1003 = 4006
        assert!((snapshot.idle - 4006.0).abs() < 0.01, "idle={}", snapshot.idle);
    }

    #[test]
    fn cpu_physical_from_node_cpu_info() {
        // 4 logical (cpu=0..3), 2 physical cores (core 0 & 1, package 0)
        let text = r#"node_cpu_seconds_total{cpu="0",mode="idle"} 100.0
node_cpu_seconds_total{cpu="1",mode="idle"} 100.0
node_cpu_seconds_total{cpu="2",mode="idle"} 100.0
node_cpu_seconds_total{cpu="3",mode="idle"} 100.0
node_cpu_info{cpu="0",core="0",package="0"} 1
node_cpu_info{cpu="1",core="1",package="0"} 1
node_cpu_info{cpu="2",core="0",package="0"} 1
node_cpu_info{cpu="3",core="1",package="0"} 1
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.cpu_logical, 4);
        // Two unique core:package pairs → 2 physical cores
        assert_eq!(metrics.cpu_physical, Some(2));
    }

    // ── DRM VRAM total parsing ────────────────────────────────────────────

    #[test]
    fn drm_vram_total_bytes_converted_to_mb() {
        // 8 GiB VRAM = 8589934592 bytes = 8192 MiB
        let text = r#"node_drm_memory_vram_total_bytes{card="card0"} 8589934592
node_drm_memory_vram_used_bytes{card="card0"} 1073741824
node_drm_gpu_busy_percent{card="card0"} 42
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        let gpu = &metrics.gpus[0];
        assert_eq!(gpu.card, "card0");
        assert_eq!(gpu.vram_total_mb, Some(8192));
        assert_eq!(gpu.vram_used_mb, Some(1024));
        assert_eq!(gpu.busy_pct, Some(42.0));
    }

    #[test]
    fn drm_vram_size_bytes_accepted_as_fallback() {
        // node_drm_memory_vram_size_bytes (older node-exporter spelling)
        let text = r#"node_drm_memory_vram_size_bytes{card="card0"} 4294967296
node_drm_gpu_busy_percent{card="card0"} 10
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        assert_eq!(metrics.gpus[0].vram_total_mb, Some(4096));
    }

    #[test]
    fn drm_vram_total_overwrites_size_when_both_present() {
        // total_bytes wins over size_bytes if both are present
        let text = r#"node_drm_memory_vram_size_bytes{card="card0"} 1073741824
node_drm_memory_vram_total_bytes{card="card0"} 4294967296
node_drm_gpu_busy_percent{card="card0"} 5
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus[0].vram_total_mb, Some(4096)); // total wins
    }

    // ── Temperature parsing from hwmon ────────────────────────────────────

    #[test]
    fn hwmon_amdgpu_temperatures_parsed() {
        let text = r#"node_hwmon_chip_names{chip="amdgpu-pci-0300",chip_name="amdgpu"} 1
node_hwmon_temp_celsius{chip="amdgpu-pci-0300",sensor="temp1"} 58.0
node_hwmon_temp_celsius{chip="amdgpu-pci-0300",sensor="temp2"} 72.0
node_hwmon_temp_celsius{chip="amdgpu-pci-0300",sensor="temp3"} 65.0
node_hwmon_power_average_watts{chip="amdgpu-pci-0300"} 145.5
node_drm_gpu_busy_percent{card="card0"} 80
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        let gpu = &metrics.gpus[0];
        assert_eq!(gpu.temp_c, Some(58.0));
        assert_eq!(gpu.temp_junction_c, Some(72.0));
        assert_eq!(gpu.temp_mem_c, Some(65.0));
        assert_eq!(gpu.power_w, Some(145.5));
    }

    #[test]
    fn hwmon_apu_chip_name_lookup_used_for_non_amdgpu_label() {
        // AMD APU: chip label is PCI address, not "amdgpu-pci-*"
        let text = r#"node_hwmon_chip_names{chip="0000:00:08_1_0000:c4:00_0",chip_name="amdgpu"} 1
node_hwmon_temp_celsius{chip="0000:00:08_1_0000:c4:00_0",sensor="temp1"} 45.0
node_hwmon_temp_celsius{chip="0000:00:08_1_0000:c4:00_0",sensor="temp2"} 60.0
node_drm_gpu_busy_percent{card="card1"} 15
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        assert_eq!(metrics.gpus[0].card, "card1");
        assert_eq!(metrics.gpus[0].temp_c, Some(45.0));
        assert_eq!(metrics.gpus[0].temp_junction_c, Some(60.0));
    }

    #[test]
    fn hwmon_power_average_watt_without_s_accepted() {
        // Some node-exporter versions emit "watt" (no trailing 's')
        let text = r#"node_hwmon_chip_names{chip="amdgpu-pci-0400",chip_name="amdgpu"} 1
node_hwmon_power_average_watt{chip="amdgpu-pci-0400"} 200.0
node_drm_gpu_busy_percent{card="card0"} 50
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus[0].power_w, Some(200.0));
    }

    // ── Empty / missing input → zero-value defaults ───────────────────────

    #[test]
    fn empty_input_returns_zero_defaults() {
        let (metrics, snapshot) = parse_prometheus_metrics("");
        assert_eq!(metrics.mem_total_mb, 0);
        assert_eq!(metrics.mem_available_mb, 0);
        assert_eq!(metrics.cpu_logical, 0);
        assert_eq!(metrics.cpu_physical, None);
        assert!(metrics.gpus.is_empty());
        assert!(metrics.scrape_ok); // scrape_ok is always true in parser
        assert_eq!(snapshot.idle, 0.0);
        assert_eq!(snapshot.total, 0.0);
    }

    #[test]
    fn comment_and_blank_lines_ignored() {
        let text = "# HELP node_memory_MemTotal_bytes Total memory\n\
                    # TYPE node_memory_MemTotal_bytes gauge\n\
                    \n\
                    node_memory_MemTotal_bytes 1073741824\n";
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.mem_total_mb, 1024);
    }

    // ── APU case: drm VRAM small but mem_available large ─────────────────

    #[test]
    fn apu_drm_vram_small_mem_available_large() {
        // AMD Ryzen AI 395+: DRM reports ~2 GiB VRAM, system has 64 GiB RAM
        let text = r#"node_memory_MemTotal_bytes 68719476736
node_memory_MemAvailable_bytes 60129542144
node_drm_memory_vram_total_bytes{card="card1"} 2147483648
node_drm_memory_vram_used_bytes{card="card1"} 1073741824
node_drm_gpu_busy_percent{card="card1"} 30
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        // mem_total = 64 GiB = 65536 MiB
        assert_eq!(metrics.mem_total_mb, 65536);
        // mem_available = ~57344 MiB
        assert_eq!(metrics.mem_available_mb, 57344);
        // DRM VRAM total = 2 GiB = 2048 MiB (small)
        assert_eq!(metrics.gpus.len(), 1);
        assert_eq!(metrics.gpus[0].vram_total_mb, Some(2048));
        // The large mem_available is available for APU unified-memory inference
        assert!(
            metrics.mem_available_mb > metrics.gpus[0].vram_total_mb.unwrap_or(0) as u64 * 10,
            "mem_available should be much larger than drm vram for APU"
        );
    }

    // ── CPU snapshot accumulation ──────────────────────────────────────────

    #[test]
    fn cpu_snapshot_sums_all_modes_and_idle() {
        let text = r#"node_cpu_seconds_total{cpu="0",mode="idle"} 500.0
node_cpu_seconds_total{cpu="0",mode="user"} 100.0
node_cpu_seconds_total{cpu="0",mode="system"} 50.0
node_cpu_seconds_total{cpu="1",mode="idle"} 600.0
node_cpu_seconds_total{cpu="1",mode="user"} 80.0
"#;
        let (_, snapshot) = parse_prometheus_metrics(text);
        // idle = 500 + 600 = 1100
        assert!((snapshot.idle - 1100.0).abs() < 0.01, "idle={}", snapshot.idle);
        // total = 500 + 100 + 50 + 600 + 80 = 1330
        assert!((snapshot.total - 1330.0).abs() < 0.01, "total={}", snapshot.total);
    }

    // ── hwmon-only fallback (no DRM collector) ────────────────────────────

    #[test]
    fn hwmon_only_no_drm_creates_synthetic_card() {
        // When no DRM metrics exist but hwmon amdgpu chip is present,
        // the parser creates a synthetic "card0" entry via the fallback path.
        let text = r#"node_hwmon_chip_names{chip="amdgpu-pci-0300",chip_name="amdgpu"} 1
node_hwmon_temp_celsius{chip="amdgpu-pci-0300",sensor="temp1"} 55.0
node_hwmon_power_average_watts{chip="amdgpu-pci-0300"} 90.0
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        let gpu = &metrics.gpus[0];
        assert_eq!(gpu.card, "card0"); // synthetic name from hwmon-only fallback
        assert_eq!(gpu.temp_c, Some(55.0));
        assert_eq!(gpu.power_w, Some(90.0));
        assert_eq!(gpu.vram_total_mb, None); // no DRM data
        assert_eq!(gpu.vram_used_mb, None);
        assert_eq!(gpu.busy_pct, None);
    }

    // ── macOS memory metrics ─────────────────────────────────────────────

    #[test]
    fn macos_memory_metrics_parsed() {
        let text = "node_memory_total_bytes 5.1539607552e+10\n\
                    node_memory_free_bytes 1.1229134848e+10\n";
        let (metrics, _) = parse_prometheus_metrics(text);
        // 51539607552 / 1048576 ≈ 49152 MiB
        assert_eq!(metrics.mem_total_mb, 49152);
        // 11229134848 / 1048576 ≈ 10710 MiB
        assert!(metrics.mem_available_mb > 10000);
    }

    // ── NVIDIA GPU via nouveau hwmon ─────────────────────────────────────

    #[test]
    fn nvidia_nouveau_hwmon_temp_parsed() {
        // NVIDIA GPU with nouveau open-source driver — temp only, no power/DRM
        let text = r#"node_hwmon_chip_names{chip="nouveau-pci-0100",chip_name="nouveau"} 1
node_hwmon_temp_celsius{chip="nouveau-pci-0100",sensor="temp1"} 62.0
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        let gpu = &metrics.gpus[0];
        assert_eq!(gpu.card, "card0");
        assert_eq!(gpu.temp_c, Some(62.0));
        assert_eq!(gpu.power_w, None); // nouveau doesn't expose power
        assert_eq!(gpu.vram_total_mb, None); // no DRM for NVIDIA
    }

    #[test]
    fn nvidia_nouveau_chip_name_map_lookup() {
        // PCI address chip label with chip_name="nouveau" in map
        let text = r#"node_hwmon_chip_names{chip="0000:01:00_0",chip_name="nouveau"} 1
node_hwmon_temp_celsius{chip="0000:01:00_0",sensor="temp1"} 55.0
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        assert_eq!(metrics.gpus[0].temp_c, Some(55.0));
    }

    // ── Intel GPU via i915/xe hwmon ──────────────────────────────────────

    #[test]
    fn intel_i915_hwmon_temp_parsed() {
        let text = r#"node_hwmon_chip_names{chip="i915-pci-0200",chip_name="i915"} 1
node_hwmon_temp_celsius{chip="i915-pci-0200",sensor="temp1"} 48.0
node_hwmon_power_average_watts{chip="i915-pci-0200"} 25.0
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        let gpu = &metrics.gpus[0];
        assert_eq!(gpu.temp_c, Some(48.0));
        assert_eq!(gpu.power_w, Some(25.0));
    }

    #[test]
    fn intel_xe_hwmon_temp_parsed() {
        // Intel Arc discrete GPU with xe kernel driver
        let text = r#"node_hwmon_chip_names{chip="xe-pci-0300",chip_name="xe"} 1
node_hwmon_temp_celsius{chip="xe-pci-0300",sensor="temp1"} 52.0
node_hwmon_power_average_watt{chip="xe-pci-0300"} 75.0
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert_eq!(metrics.gpus.len(), 1);
        let gpu = &metrics.gpus[0];
        assert_eq!(gpu.temp_c, Some(52.0));
        assert_eq!(gpu.power_w, Some(75.0));
    }

    // ── Multi-vendor mixed system ────────────────────────────────────────

    #[test]
    fn multi_vendor_gpus_all_collected() {
        // System with AMD + NVIDIA GPUs
        let text = r#"node_hwmon_chip_names{chip="amdgpu-pci-0300",chip_name="amdgpu"} 1
node_hwmon_chip_names{chip="nouveau-pci-0400",chip_name="nouveau"} 1
node_hwmon_temp_celsius{chip="amdgpu-pci-0300",sensor="temp1"} 58.0
node_hwmon_temp_celsius{chip="amdgpu-pci-0300",sensor="temp2"} 72.0
node_hwmon_temp_celsius{chip="nouveau-pci-0400",sensor="temp1"} 65.0
node_hwmon_power_average_watts{chip="amdgpu-pci-0300"} 145.0
node_drm_gpu_busy_percent{card="card0"} 80
node_drm_memory_vram_used_bytes{card="card0"} 4294967296
node_drm_memory_vram_total_bytes{card="card0"} 8589934592
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        // DRM card (AMD) + hwmon-only nouveau = 1 DRM card but 2 hwmon chips
        // DRM path takes priority; nouveau falls into hwmon at position 1
        assert!(!metrics.gpus.is_empty());
        // AMD card has DRM data
        let amd = &metrics.gpus[0];
        assert_eq!(amd.card, "card0");
        assert_eq!(amd.vram_total_mb, Some(8192));
        assert_eq!(amd.busy_pct, Some(80.0));
    }

    // ── macOS: no GPU metrics, memory + CPU only ─────────────────────────

    #[test]
    fn macos_full_output_no_gpu() {
        // macOS has no hwmon or DRM — only memory + CPU
        let text = r#"node_memory_total_bytes 5.1539607552e+10
node_memory_free_bytes 1.1229134848e+10
node_memory_active_bytes 1.608425472e+10
node_memory_wired_bytes 3.743793152e+09
node_cpu_seconds_total{cpu="0",mode="idle"} 50000.0
node_cpu_seconds_total{cpu="0",mode="user"} 10000.0
node_cpu_seconds_total{cpu="1",mode="idle"} 51000.0
node_cpu_seconds_total{cpu="1",mode="user"} 9000.0
"#;
        let (metrics, _) = parse_prometheus_metrics(text);
        assert!(metrics.mem_total_mb > 40000); // ~49 GiB
        assert!(metrics.mem_available_mb > 10000);
        assert_eq!(metrics.cpu_logical, 2);
        assert!(metrics.gpus.is_empty()); // no GPU on macOS node-exporter
    }

    // ── HwMetrics struct methods ──────────────────────────────────────────

    #[test]
    fn vram_free_mb_subtracts_used_from_total() {
        let m = HwMetrics { vram_total_mb: 8192, vram_used_mb: 2048, ..Default::default() };
        assert_eq!(m.vram_free_mb(), 6144);
    }

    #[test]
    fn vram_free_mb_zero_when_fully_used() {
        let m = HwMetrics { vram_total_mb: 4096, vram_used_mb: 4096, ..Default::default() };
        assert_eq!(m.vram_free_mb(), 0);
    }

    #[test]
    fn vram_free_mb_negative_on_overcommit() {
        // VramPool may transiently overcommit — negative result is valid.
        let m = HwMetrics { vram_total_mb: 1000, vram_used_mb: 1200, ..Default::default() };
        assert_eq!(m.vram_free_mb(), -200);
    }

    #[test]
    fn max_temp_c_returns_junction_when_highest() {
        // junction (temp2) is typically the hottest for AMD GPUs
        let m = HwMetrics { temp_c: 58.0, temp_junction_c: 72.0, temp_mem_c: 65.0, ..Default::default() };
        assert_eq!(m.max_temp_c(), 72.0);
    }

    #[test]
    fn max_temp_c_returns_edge_when_highest() {
        let m = HwMetrics { temp_c: 90.0, temp_junction_c: 72.0, temp_mem_c: 65.0, ..Default::default() };
        assert_eq!(m.max_temp_c(), 90.0);
    }

    #[test]
    fn max_temp_c_returns_mem_when_highest() {
        let m = HwMetrics { temp_c: 58.0, temp_junction_c: 72.0, temp_mem_c: 80.0, ..Default::default() };
        assert_eq!(m.max_temp_c(), 80.0);
    }

    #[test]
    fn max_temp_c_all_zero_by_default() {
        let m = HwMetrics::default();
        assert_eq!(m.max_temp_c(), 0.0);
    }

    #[test]
    fn is_overheating_false_below_threshold() {
        // 84.9 °C is below the 85 °C threshold
        let m = HwMetrics { temp_junction_c: 84.9, ..Default::default() };
        assert!(!m.is_overheating());
    }

    #[test]
    fn is_overheating_true_at_exactly_85() {
        let m = HwMetrics { temp_junction_c: 85.0, ..Default::default() };
        assert!(m.is_overheating());
    }

    #[test]
    fn is_overheating_triggered_by_mem_sensor() {
        // edge + junction below threshold, mem at 85 → still overheating
        let m = HwMetrics { temp_c: 60.0, temp_junction_c: 72.0, temp_mem_c: 85.0, ..Default::default() };
        assert!(m.is_overheating());
    }

    #[test]
    fn is_overheating_false_when_all_zero() {
        assert!(!HwMetrics::default().is_overheating());
    }
}
