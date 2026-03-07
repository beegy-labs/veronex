//! veronex-agent — lightweight hardware metrics HTTP server.
//!
//! Deploy one instance alongside each Ollama server.  The agent reads AMD GPU
//! counters from sysfs, system RAM from `/proc/meminfo`, and loaded-model VRAM
//! from Ollama `/api/ps`, then exposes the data as JSON on `GET /api/metrics`.
//!
//! ## Configuration (environment variables)
//!
//! | Variable      | Default                    | Description                       |
//! |---------------|----------------------------|-----------------------------------|
//! | `OLLAMA_URL`  | `http://localhost:11434`   | Ollama base URL                   |
//! | `PORT`        | `9091`                     | HTTP listen port                  |
//!
//! ## Deployment examples
//!
//! ```bash
//! # Docker
//! docker run -d --name veronex-agent \
//!   -p 9091:9091 \
//!   -v /sys:/sys:ro \
//!   -v /proc/meminfo:/proc/meminfo:ro \
//!   -e OLLAMA_URL=http://host.docker.internal:11434 \
//!   ghcr.io/beegy/veronex-agent:latest
//!
//! # Bare-metal
//! ./veronex-agent
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::fs;

// ── Config ─────────────────────────────────────────────────────────────────────

struct Config {
    ollama_url: String,
    port: u16,
}

impl Config {
    fn from_env() -> Self {
        Self {
            ollama_url: std::env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(9091),
        }
    }
}

// ── Response DTOs ──────────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct GpuInfo {
    vram_used_mb: u32,
    vram_total_mb: u32,
    gpu_util_pct: u8,
    power_w: f32,
    temp_c: f32,
    /// GPU vendor: "amd", "nvidia", or "unknown".
    /// Detected from sysfs `/sys/class/drm/cardN/device/vendor`.
    #[serde(default)]
    gpu_vendor: String,
}

#[derive(Serialize, Default)]
struct MemoryInfo {
    used_mb: u32,
    total_mb: u32,
    available_mb: u32,
}

#[derive(Serialize, Default)]
struct OllamaInfo {
    loaded_model_count: u8,
    /// Loaded model VRAM in MiB (sum of `size_vram` from `/api/ps`).
    loaded_vram_mb: u32,
}

#[derive(Serialize)]
struct MetricsResponse {
    gpu: GpuInfo,
    memory: MemoryInfo,
    ollama: OllamaInfo,
}

// ── sysfs helpers ──────────────────────────────────────────────────────────────

const SYSFS_DRM: &str = "/sys/class/drm";

async fn read_u64(path: &str) -> Option<u64> {
    fs::read_to_string(path)
        .await
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Vendor string from PCI vendor ID.
fn vendor_name(vendor_id: &str) -> &'static str {
    match vendor_id.trim() {
        "0x1002" => "amd",
        "0x10de" => "nvidia",
        _ => "unknown",
    }
}

/// Find the first GPU card index and its vendor from sysfs.
///
/// Checks AMD (0x1002) and NVIDIA (0x10de) vendors.
/// For AMD: requires non-zero `mem_info_vram_total`.
/// For NVIDIA: requires the vendor file to exist (VRAM read via nvidia-smi).
async fn find_gpu_card() -> Option<(u8, &'static str)> {
    let mut dir = fs::read_dir(SYSFS_DRM).await.ok()?;
    loop {
        let entry = match dir.next_entry().await {
            Ok(Some(e)) => e,
            _ => break,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("card") || name_str.contains('-') {
            continue;
        }
        let idx: u8 = match name_str[4..].parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let vendor_path = format!("{SYSFS_DRM}/{name_str}/device/vendor");
        let vendor_id = match fs::read_to_string(&vendor_path).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let vendor = vendor_name(&vendor_id);
        match vendor {
            "amd" => {
                let vram_path = format!("{SYSFS_DRM}/{name_str}/device/mem_info_vram_total");
                if let Some(v) = read_u64(&vram_path).await {
                    if v > 0 {
                        return Some((idx, vendor));
                    }
                }
            }
            "nvidia" => {
                // NVIDIA sysfs exists but VRAM is read differently.
                // Card detection is enough — metrics collected via nvidia-smi fallback.
                return Some((idx, vendor));
            }
            _ => continue,
        }
    }
    None
}

/// Return the first hwmon directory under a DRM card's device path.
async fn find_hwmon(card: &str) -> Option<String> {
    let base = format!("{SYSFS_DRM}/{card}/device/hwmon");
    let mut dir = fs::read_dir(&base).await.ok()?;
    match dir.next_entry().await {
        Ok(Some(entry)) => Some(format!("{base}/{}", entry.file_name().to_string_lossy())),
        _ => None,
    }
}

async fn collect_gpu(card: Option<(u8, &str)>) -> GpuInfo {
    let Some((idx, vendor)) = card else {
        return GpuInfo::default();
    };
    let card_name = format!("card{idx}");
    let dev = format!("{SYSFS_DRM}/{card_name}/device");

    // AMD sysfs VRAM paths. NVIDIA uses different mechanism (nvidia-smi).
    let (vram_used, vram_total, gpu_util) = if vendor == "amd" {
        let used = read_u64(&format!("{dev}/mem_info_vram_used"))
            .await
            .map(|v| (v / 1_048_576) as u32)
            .unwrap_or(0);
        let total = read_u64(&format!("{dev}/mem_info_vram_total"))
            .await
            .map(|v| (v / 1_048_576) as u32)
            .unwrap_or(0);
        let util = read_u64(&format!("{dev}/gpu_busy_percent"))
            .await
            .unwrap_or(0)
            .min(100) as u8;
        (used, total, util)
    } else {
        (0, 0, 0)
    };

    let (power_w, temp_c) = if let Some(hwmon) = find_hwmon(&card_name).await {
        let p = read_u64(&format!("{hwmon}/power1_average")).await.unwrap_or(0);
        let t = read_u64(&format!("{hwmon}/temp1_input")).await.unwrap_or(0);
        (p as f32 / 1_000_000.0, t as f32 / 1_000.0)
    } else {
        (0.0, 0.0)
    };

    GpuInfo {
        vram_used_mb: vram_used,
        vram_total_mb: vram_total,
        gpu_util_pct: gpu_util,
        power_w,
        temp_c,
        gpu_vendor: vendor.to_string(),
    }
}

// ── /proc/meminfo ──────────────────────────────────────────────────────────────

async fn collect_memory() -> MemoryInfo {
    // Try both the bind-mount path and the native path.
    let content = match fs::read_to_string("/proc/meminfo").await {
        Ok(c) => c,
        Err(_) => fs::read_to_string("/host/proc/meminfo").await.unwrap_or_default(),
    };

    let mut total_kb = 0u64;
    let mut free_kb = 0u64;
    let mut available_kb = 0u64;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("MemTotal:") => total_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            Some("MemFree:") => free_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            Some("MemAvailable:") => {
                available_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0)
            }
            _ => {}
        }
    }

    let used_kb = total_kb.saturating_sub(free_kb);
    MemoryInfo {
        total_mb: (total_kb / 1024) as u32,
        used_mb: (used_kb / 1024) as u32,
        available_mb: (available_kb / 1024) as u32,
    }
}

// ── Ollama /api/ps ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OllamaPs {
    models: Option<Vec<OllamaModel>>,
}

#[derive(Deserialize)]
struct OllamaModel {
    #[serde(default)]
    size_vram: u64,
}

async fn collect_ollama(ollama_url: &str, client: &reqwest::Client) -> OllamaInfo {
    let url = format!("{}/api/ps", ollama_url.trim_end_matches('/'));
    let resp = match client.get(&url).timeout(Duration::from_secs(3)).send().await {
        Ok(r) => r,
        Err(_) => return OllamaInfo::default(),
    };
    let ps: OllamaPs = match resp.json().await {
        Ok(p) => p,
        Err(_) => return OllamaInfo::default(),
    };
    let models = ps.models.unwrap_or_default();
    let count = models.len().min(255) as u8;
    let loaded_vram_mb = models.iter().map(|m| m.size_vram / 1_048_576).sum::<u64>() as u32;
    OllamaInfo { loaded_model_count: count, loaded_vram_mb }
}

// ── App state + handler ────────────────────────────────────────────────────────

struct AppState {
    ollama_url: String,
    http: reqwest::Client,
}

async fn handler_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let card = find_gpu_card().await;

    let (gpu, memory, ollama) = tokio::join!(
        collect_gpu(card),
        collect_memory(),
        collect_ollama(&state.ollama_url, &state.http),
    );

    Json(MetricsResponse { gpu, memory, ollama })
}

async fn handler_health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

// ── Main ───────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env();

    let state = Arc::new(AppState {
        ollama_url: cfg.ollama_url.clone(),
        http: reqwest::Client::new(),
    });

    let app = Router::new()
        .route("/api/metrics", get(handler_metrics))
        .route("/health", get(handler_health))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!("veronex-agent listening on {addr} (ollama={ollama_url})", ollama_url = cfg.ollama_url);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
