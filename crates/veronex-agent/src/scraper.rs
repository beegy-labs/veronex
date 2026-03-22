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

/// Max response body size from node-exporter (16 MiB).
const MAX_NODE_EXPORTER_BODY: usize = 16 * 1024 * 1024;

/// Max response body size from Ollama /api/ps (1 MiB).
const MAX_OLLAMA_BODY: usize = 1024 * 1024;

/// Max labels per metric line (DOS protection).
const MAX_LABELS: usize = 32;

/// Max models from Ollama /api/ps (DOS protection).
const MAX_OLLAMA_MODELS: usize = 256;

/// Metric name prefixes to forward from node-exporter.
/// Everything else is dropped at the agent level.
const NODE_EXPORTER_ALLOWLIST: &[&str] = &[
    // Linux memory
    "node_memory_MemTotal_bytes",
    "node_memory_MemAvailable_bytes",
    // macOS memory
    "node_memory_total_bytes",
    "node_memory_free_bytes",
    // CPU utilisation (mode-filtered separately via CPU_MODE_ALLOWLIST)
    "node_cpu_seconds_total",
    // GPU (AMD DRM)
    "node_drm_",
    // Hardware sensors — temperature, power, chip identification, sensor labels
    "node_hwmon_temp_celsius",
    "node_hwmon_power_average_watt",
    "node_hwmon_power_watt",
    // chip_names: maps chip→vendor (amdgpu, coretemp, k10temp, nouveau, nvme, etc.)
    "node_hwmon_chip_names",
    // sensor_label: maps sensor→human-readable name (edge, junction, Tctl, Package id 0, etc.)
    "node_hwmon_sensor_label",
];

/// CPU modes worth tracking. All others (nice, irq, softirq, steal, guest,
/// guest_nice) are dropped — they add ~55% of cpu row volume with no benefit
/// for GPU-server monitoring.
const CPU_MODE_ALLOWLIST: &[&str] = &["user", "system", "iowait", "idle"];

fn is_allowed(name: &str) -> bool {
    NODE_EXPORTER_ALLOWLIST.iter().any(|prefix| name.starts_with(prefix))
}

/// Returns false when the metric is `node_cpu_seconds_total` and its `mode`
/// label is not in CPU_MODE_ALLOWLIST. All other metrics pass through.
fn is_cpu_mode_allowed(name: &str, labels: &[(String, String)]) -> bool {
    if name != "node_cpu_seconds_total" {
        return true;
    }
    labels
        .iter()
        .find(|(k, _)| k == "mode")
        .map(|(_, v)| CPU_MODE_ALLOWLIST.contains(&v.as_str()))
        .unwrap_or(false)
}

/// A single gauge metric — raw name, raw value, raw labels from the source.
pub struct Gauge {
    pub name: String,
    pub value: f64,
    pub labels: Vec<(String, String)>,
}

// ── Hardware classification ─────────────────────────────────────────────────
// Maps node-exporter chip_name to normalized hw_type + hw_vendor labels.
// Agent is the ONLY component that does this classification — downstream
// (OTel → Redpanda → ClickHouse) is pure data pipeline.

/// Classify a chip_name into (hw_type, hw_vendor).
/// Returns None for unrecognized/irrelevant chips (NIC, board sensors, etc.)
fn classify_chip(chip_name: &str) -> Option<(&'static str, &'static str)> {
    match chip_name {
        // GPU drivers
        "amdgpu"                            => Some(("gpu", "amd")),
        "nouveau"                           => Some(("gpu", "nvidia")),
        // CPU temperature drivers
        "coretemp"                          => Some(("cpu", "intel")),
        "k10temp" | "zenpower" | "zenergy"  => Some(("cpu", "amd")),
        // Storage
        "nvme"                              => Some(("storage", "nvme")),
        // ACPI thermal zone (board-level CPU temp fallback)
        "acpitz"                            => Some(("board", "acpi")),
        // Motherboard super I/O chips (Nuvoton, ITE, ASUS, etc.)
        n if n.starts_with("nct") || n.starts_with("it87") || n.starts_with("asus") => Some(("board", "motherboard")),
        _ => None,
    }
}

/// Normalize a sensor label to a canonical role name.
/// This maps hardware-specific labels to common keys usable in analytics.
fn normalize_sensor_role(hw_type: &str, label: &str) -> &'static str {
    let lower = label.to_ascii_lowercase();
    match hw_type {
        "gpu" => {
            if lower.contains("junction")                    { "temp_junction" }
            else if lower.contains("mem")                    { "temp_mem" }
            else if lower.contains("edge") || lower == "temp1" { "temp_edge" }
            else if lower.contains("ppt") || lower.contains("power") { "power" }
            else { "other" }
        }
        "cpu" => {
            // AMD: Tctl, Tdie, Tccd1, Tccd2...
            // Intel: Package id 0, Core 0, Core 1...
            if lower.contains("package") || lower == "tctl" || lower == "tdie" { "temp_package" }
            else if lower.starts_with("core") || lower.starts_with("tccd")    { "temp_core" }
            else { "temp_package" } // default to package for unknown CPU sensors
        }
        _ => "other",
    }
}

/// Enrich hwmon gauges with `hw_type`, `hw_vendor`, `hw_role` labels
/// based on chip_name classification and sensor_label mapping.
pub fn enrich_hwmon_labels(
    gauges: &mut [Gauge],
    chip_map: &std::collections::HashMap<String, String>,  // chip → chip_name
    label_map: &std::collections::HashMap<(String, String), String>, // (chip, sensor) → label
) {
    for gauge in gauges.iter_mut() {
        // Only enrich hwmon metrics (temp, power)
        if !gauge.name.starts_with("node_hwmon_temp") && !gauge.name.starts_with("node_hwmon_power") {
            continue;
        }

        let chip = gauge.labels.iter().find(|(k, _)| k == "chip").map(|(_, v)| v.as_str());
        let sensor = gauge.labels.iter().find(|(k, _)| k == "sensor").map(|(_, v)| v.as_str());

        let Some(chip_val) = chip else { continue };
        let Some(chip_name) = chip_map.get(chip_val) else { continue };
        let Some((hw_type, hw_vendor)) = classify_chip(chip_name) else { continue };

        // Resolve sensor label for role classification
        let role = if let Some(sensor_val) = sensor {
            if let Some(label) = label_map.get(&(chip_val.to_string(), sensor_val.to_string())) {
                normalize_sensor_role(hw_type, label)
            } else {
                normalize_sensor_role(hw_type, sensor_val)
            }
        } else {
            "other"
        };

        gauge.labels.push(("hw_type".to_string(), hw_type.to_string()));
        gauge.labels.push(("hw_vendor".to_string(), hw_vendor.to_string()));
        gauge.labels.push(("hw_role".to_string(), role.to_string()));
    }
}

// ── Node-exporter ────────────────────────────────────────────────────────────

/// Scrape node-exporter /metrics — select allowed metrics, forward raw values.
/// Enriches hwmon metrics with `hw_type`, `hw_vendor`, `hw_role` labels
/// based on chip_name classification (agent-side only — pipeline is raw).
pub async fn scrape_node_exporter(client: &reqwest::Client, base_url: &str) -> Vec<Gauge> {
    let url = format!("{}/metrics", base_url.trim_end_matches('/'));
    match client.get(&url).timeout(SCRAPE_TIMEOUT).send().await {
        Ok(resp) => {
            let content_len = resp.content_length().unwrap_or(0) as usize;
            if content_len > MAX_NODE_EXPORTER_BODY {
                tracing::warn!(url, bytes = content_len, "node-exporter body too large, skipping");
                return vec![];
            }
            match resp.bytes().await {
                Ok(bytes) if bytes.len() > MAX_NODE_EXPORTER_BODY => {
                    tracing::warn!(url, bytes = bytes.len(), "node-exporter body exceeded limit");
                    vec![]
                }
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut gauges = parse_node_exporter(&text);

                    // Build chip_name and sensor_label maps from annotation metrics
                    let mut chip_map = std::collections::HashMap::new();
                    let mut label_map = std::collections::HashMap::new();
                    for g in &gauges {
                        if g.name == "node_hwmon_chip_names" {
                            let chip = g.labels.iter().find(|(k, _)| k == "chip").map(|(_, v)| v.clone());
                            let cn = g.labels.iter().find(|(k, _)| k == "chip_name").map(|(_, v)| v.clone());
                            if let (Some(c), Some(n)) = (chip, cn) {
                                chip_map.insert(c, n);
                            }
                        } else if g.name == "node_hwmon_sensor_label" {
                            let chip = g.labels.iter().find(|(k, _)| k == "chip").map(|(_, v)| v.clone());
                            let sensor = g.labels.iter().find(|(k, _)| k == "sensor").map(|(_, v)| v.clone());
                            let label = g.labels.iter().find(|(k, _)| k == "label").map(|(_, v)| v.clone());
                            if let (Some(c), Some(s), Some(l)) = (chip, sensor, label) {
                                label_map.insert((c, s), l);
                            }
                        }
                    }

                    enrich_hwmon_labels(&mut gauges, &chip_map, &label_map);
                    gauges
                }
                Err(e) => {
                    tracing::debug!("node-exporter read failed: {e}");
                    vec![]
                }
            }
        }
        Err(e) => {
            tracing::debug!("node-exporter scrape failed: {e}");
            vec![]
        }
    }
}

/// Parse Prometheus text — filter by allowlist, skip NaN/Inf, pass raw values.
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

        if !value.is_finite() {
            continue;
        }

        let name = match metric_part.find('{') {
            Some(i) => &metric_part[..i],
            None => metric_part,
        };

        if !is_allowed(name) {
            continue;
        }

        let labels = parse_labels(metric_part);
        if !is_cpu_mode_allowed(name, &labels) {
            continue;
        }
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
    // Find the boundary between metric{labels} and the value.
    // If braces exist, split after '}'; otherwise split at first space.
    let split_at = if let Some(brace_end) = line.find('}') {
        line[brace_end..].find(' ').map(|i| brace_end + i)
    } else {
        line.find(' ')
    }?;
    let metric_part = &line[..split_at];
    let value: f64 = line[split_at + 1..].split_whitespace().next()?.parse().ok()?;
    Some((metric_part, value))
}

/// Extract labels from `name{k1="v1",k2="v2"}`, capped at MAX_LABELS.
fn parse_labels(metric_part: &str) -> Vec<(String, String)> {
    let Some(start) = metric_part.find('{') else {
        return vec![];
    };
    let Some(end) = metric_part.find('}') else {
        return vec![];
    };
    let inner = &metric_part[start + 1..end];
    let mut labels = Vec::new();
    let mut remaining = inner;

    while !remaining.is_empty() && labels.len() < MAX_LABELS {
        // key="value"
        let Some(eq) = remaining.find('=') else { break };
        let key = &remaining[..eq];
        let after_eq = &remaining[eq + 1..];

        if !after_eq.starts_with('"') {
            break;
        }
        let Some(val_end) = after_eq[1..].find('"') else {
            break;
        };
        let val = &after_eq[1..1 + val_end];
        labels.push((key.to_string(), val.to_string()));

        // Advance past: key="value" → skip eq + opening quote + value + closing quote
        let advance = eq + 1 + 1 + val_end + 1;
        remaining = &remaining[advance..];
        if remaining.starts_with(',') {
            remaining = &remaining[1..];
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
pub struct OllamaPsModel {
    pub name: Option<String>,
    pub size_vram: Option<u64>,
    pub size: Option<u64>,
}

/// Scrape Ollama /api/ps — returns raw model list (capped at MAX_OLLAMA_MODELS).
/// Returns empty vec on any error.
pub async fn scrape_ollama_raw(client: &reqwest::Client, base_url: &str) -> Vec<OllamaPsModel> {
    let url = format!("{}/api/ps", base_url.trim_end_matches('/'));
    let resp: OllamaPsResponse = match client.get(&url).timeout(SCRAPE_TIMEOUT).send().await {
        Ok(r) => {
            let content_len = r.content_length().unwrap_or(0) as usize;
            if content_len > MAX_OLLAMA_BODY {
                tracing::warn!(url, bytes = content_len, "ollama body too large, skipping");
                return vec![];
            }
            match r.bytes().await {
                Ok(bytes) if bytes.len() > MAX_OLLAMA_BODY => {
                    tracing::warn!(url, bytes = bytes.len(), "ollama body exceeded limit");
                    return vec![];
                }
                Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!("ollama parse failed: {e}");
                        return vec![];
                    }
                },
                Err(e) => {
                    tracing::debug!("ollama read failed: {e}");
                    return vec![];
                }
            }
        }
        Err(e) => {
            tracing::debug!("ollama scrape failed: {e}");
            return vec![];
        }
    };
    let mut models = resp.models.unwrap_or_default();
    if models.len() > MAX_OLLAMA_MODELS {
        tracing::warn!(count = models.len(), "ollama returned too many models, truncating");
        models.truncate(MAX_OLLAMA_MODELS);
    }
    models
}

/// Convert raw OllamaPsModel list to OTLP gauges.
pub fn ollama_gauges_from_raw(models: &[OllamaPsModel]) -> Vec<Gauge> {
    let mut gauges = Vec::with_capacity(models.len() * 2 + 1);

    gauges.push(Gauge {
        name: "ollama_loaded_models".into(),
        value: models.len() as f64,
        labels: vec![],
    });

    for model in models {
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

/// Scrape Ollama /api/ps — forward raw byte values, no conversion.
pub async fn scrape_ollama(client: &reqwest::Client, base_url: &str) -> Vec<Gauge> {
    let models = scrape_ollama_raw(client, base_url).await;
    ollama_gauges_from_raw(&models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Concrete example: allowlist filtering with real metric data.
    #[test]
    fn filters_by_allowlist_and_preserves_raw_values() {
        let text = r#"
node_memory_MemTotal_bytes 6.7385810944e+10
node_filesystem_size_bytes{mountpoint="/"} 500000000000
node_drm_gpu_busy_percent{card="card1"} 42
some_random_metric 999
"#;
        let gauges = parse_node_exporter(text);
        assert_eq!(gauges.len(), 2);
        let mem = gauges.iter().find(|g| g.name == "node_memory_MemTotal_bytes").unwrap();
        assert!((mem.value - 6.7385810944e10).abs() < 1.0);
        assert!(gauges.iter().any(|g| g.name == "node_drm_gpu_busy_percent"));
    }

    /// Concrete edge case: NaN/Inf skipped.
    #[test]
    fn skips_nan_and_inf() {
        let text = "node_memory_MemTotal_bytes NaN\nnode_memory_MemAvailable_bytes +Inf\n";
        let gauges = parse_node_exporter(text);
        assert!(gauges.is_empty());
    }

    /// Concrete example: label parsing with real metric format.
    #[test]
    fn preserves_labels_end_to_end() {
        let text = r#"node_hwmon_temp_celsius{chip="0000:c4:00_0",sensor="temp1"} 55"#;
        let gauges = parse_node_exporter(text);
        assert_eq!(gauges.len(), 1);
        assert_eq!(gauges[0].labels, vec![
            ("chip".into(), "0000:c4:00_0".into()),
            ("sensor".into(), "temp1".into()),
        ]);
        assert!(parse_labels("foo").is_empty());
    }

    /// CPU mode filtering: allowed modes pass, blocked modes are dropped.
    #[test]
    fn cpu_mode_filtering() {
        let allowed = ["user", "system", "iowait", "idle"];
        let blocked = ["nice", "irq", "softirq", "steal", "guest", "guest_nice"];

        for mode in &allowed {
            let text = format!("node_cpu_seconds_total{{cpu=\"0\",mode=\"{mode}\"}} 1.0");
            let gauges = parse_node_exporter(&text);
            assert_eq!(gauges.len(), 1, "mode={mode} should pass");
        }
        for mode in &blocked {
            let text = format!("node_cpu_seconds_total{{cpu=\"0\",mode=\"{mode}\"}} 1.0");
            let gauges = parse_node_exporter(&text);
            assert!(gauges.is_empty(), "mode={mode} should be filtered");
        }
        // No mode label → filtered (unknown mode)
        let text = "node_cpu_seconds_total{cpu=\"0\"} 1.0";
        let gauges = parse_node_exporter(text);
        assert!(gauges.is_empty(), "missing mode label should be filtered");
    }

    /// node_hwmon_chip_names is in the allowlist (needed by analytics for GPU chip→vendor mapping).
    #[test]
    fn chip_names_allowed() {
        let text = "node_hwmon_chip_names{chip=\"0000:c4:00_0\",chip_name=\"amdgpu\"} 1";
        let gauges = parse_node_exporter(text);
        assert_eq!(gauges.len(), 1);
        assert_eq!(gauges[0].name, "node_hwmon_chip_names");
    }

    proptest! {
        /// Allowed metrics always pass through with correct value and name.
        #[test]
        fn allowed_metric_always_passes(
            prefix in prop::sample::select(vec![
                "node_memory_MemTotal_bytes",
                "node_memory_MemAvailable_bytes",
                "node_drm_gpu_busy",
                "node_hwmon_temp_celsius",
                "node_hwmon_power_average_watt",
            ]),
            value in 0.0f64..1e15,
        ) {
            prop_assume!(value.is_finite());
            let text = format!("{prefix} {value}");
            let gauges = parse_node_exporter(&text);
            prop_assert_eq!(gauges.len(), 1);
            prop_assert_eq!(&gauges[0].name, &prefix);
            prop_assert!((gauges[0].value - value).abs() < 1.0);
        }

        /// node_cpu_seconds_total with an allowed mode always passes.
        #[test]
        fn cpu_allowed_mode_passes(
            mode in prop::sample::select(vec!["user", "system", "iowait", "idle"]),
            value in 0.0f64..1e15,
        ) {
            prop_assume!(value.is_finite());
            let text = format!("node_cpu_seconds_total{{cpu=\"0\",mode=\"{mode}\"}} {value}");
            let gauges = parse_node_exporter(&text);
            prop_assert_eq!(gauges.len(), 1);
            prop_assert!((gauges[0].value - value).abs() < 1.0);
        }

        /// node_cpu_seconds_total with a blocked mode is always filtered.
        #[test]
        fn cpu_blocked_mode_filtered(
            mode in prop::sample::select(vec!["nice", "irq", "softirq", "steal", "guest", "guest_nice"]),
            value in 0.0f64..1e15,
        ) {
            prop_assume!(value.is_finite());
            let text = format!("node_cpu_seconds_total{{cpu=\"0\",mode=\"{mode}\"}} {value}");
            let gauges = parse_node_exporter(&text);
            prop_assert!(gauges.is_empty());
        }

        /// Non-allowed metrics are always filtered out.
        #[test]
        fn non_allowed_metric_filtered(
            name in "[a-z_]{5,20}",
            value in 0.0f64..1e10,
        ) {
            prop_assume!(value.is_finite());
            prop_assume!(!is_allowed(&name));
            let text = format!("{name} {value}");
            let gauges = parse_node_exporter(&text);
            prop_assert!(gauges.is_empty());
        }

        /// Comments and blank lines never produce gauges.
        #[test]
        fn comments_and_blanks_ignored(
            comment in "# [a-zA-Z ]{0,50}",
        ) {
            let text = format!("{comment}\n\n");
            let gauges = parse_node_exporter(&text);
            prop_assert!(gauges.is_empty());
        }

        /// Labels are capped at MAX_LABELS regardless of input count.
        #[test]
        fn labels_never_exceed_max(count in (MAX_LABELS + 1)..80usize) {
            let parts: Vec<String> = (0..count).map(|i| format!("k{i}=\"v{i}\"")).collect();
            let metric = format!("foo{{{}}}", parts.join(","));
            let labels = parse_labels(&metric);
            prop_assert!(labels.len() <= MAX_LABELS);
        }

        /// parse_labels roundtrip: N labels in → N labels out (up to MAX_LABELS).
        #[test]
        fn parse_labels_count_matches(count in 1usize..MAX_LABELS) {
            let parts: Vec<String> = (0..count).map(|i| format!("k{i}=\"v{i}\"")).collect();
            let metric = format!("foo{{{}}}", parts.join(","));
            let labels = parse_labels(&metric);
            prop_assert_eq!(labels.len(), count);
        }
    }

    // ── Hardware classification tests ─────────────────────────────────

    #[test]
    fn classify_chip_gpu_vendors() {
        assert_eq!(classify_chip("amdgpu"), Some(("gpu", "amd")));
        assert_eq!(classify_chip("nouveau"), Some(("gpu", "nvidia")));
    }

    #[test]
    fn classify_chip_cpu_vendors() {
        assert_eq!(classify_chip("coretemp"), Some(("cpu", "intel")));
        assert_eq!(classify_chip("k10temp"), Some(("cpu", "amd")));
        assert_eq!(classify_chip("zenpower"), Some(("cpu", "amd")));
    }

    #[test]
    fn classify_chip_storage_and_board() {
        assert_eq!(classify_chip("nvme"), Some(("storage", "nvme")));
        assert_eq!(classify_chip("acpitz"), Some(("board", "acpi")));
        assert_eq!(classify_chip("nct6775"), Some(("board", "motherboard")));
    }

    #[test]
    fn classify_chip_unknown_returns_none() {
        assert_eq!(classify_chip("r8169_0_c100:00"), None);
        assert_eq!(classify_chip("unknown_driver"), None);
    }

    #[test]
    fn normalize_gpu_sensor_roles() {
        assert_eq!(normalize_sensor_role("gpu", "edge"), "temp_edge");
        assert_eq!(normalize_sensor_role("gpu", "junction"), "temp_junction");
        assert_eq!(normalize_sensor_role("gpu", "mem"), "temp_mem");
        assert_eq!(normalize_sensor_role("gpu", "PPT"), "power");
    }

    #[test]
    fn normalize_cpu_sensor_roles() {
        assert_eq!(normalize_sensor_role("cpu", "Package id 0"), "temp_package");
        assert_eq!(normalize_sensor_role("cpu", "Tctl"), "temp_package");
        assert_eq!(normalize_sensor_role("cpu", "Core 0"), "temp_core");
        assert_eq!(normalize_sensor_role("cpu", "Tccd1"), "temp_core");
    }

    #[test]
    fn enrich_adds_hw_labels() {
        let mut gauges = vec![Gauge {
            name: "node_hwmon_temp_celsius".into(),
            value: 55.0,
            labels: vec![
                ("chip".into(), "0000:c4:00_0".into()),
                ("sensor".into(), "temp1".into()),
            ],
        }];
        let chip_map = std::collections::HashMap::from([
            ("0000:c4:00_0".into(), "amdgpu".into()),
        ]);
        let label_map = std::collections::HashMap::from([
            (("0000:c4:00_0".into(), "temp1".into()), "edge".into()),
        ]);
        enrich_hwmon_labels(&mut gauges, &chip_map, &label_map);
        let labels = &gauges[0].labels;
        assert!(labels.iter().any(|(k, v)| k == "hw_type" && v == "gpu"));
        assert!(labels.iter().any(|(k, v)| k == "hw_vendor" && v == "amd"));
        assert!(labels.iter().any(|(k, v)| k == "hw_role" && v == "temp_edge"));
    }
}
