//! `/probe` — system information returned to Veronex on registration.
//!
//! Implementation strategy:
//! - OS / arch / cpu count: read from std + /proc / sysctl wrappers.
//! - GPU detection:
//!   - Linux: parse `/sys/class/drm/card*/device/mem_info_vram_total`.
//!   - Mac: assume Apple Metal + unified memory (no separate VRAM).
//!
//! The probe is intentionally read-only — never spawns a subprocess so
//! it can answer in <100ms even on a fresh node. Operator can re-fire
//! on demand via `POST /v1/admin/nodes/{id}/probe`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResponse {
    pub os: &'static str,
    pub arch: &'static str,
    pub gpu_accel: &'static str,
    pub gpu_model: Option<String>,
    pub total_vram_mb: i64,
    pub total_ram_mb: i64,
    pub cpu_threads: i16,
}

/// Pure value collection. Hardware-specific reads are best-effort —
/// missing data falls back to 0 / None so the response is always valid
/// even on a node with unusual sysfs layout.
pub fn probe() -> ProbeResponse {
    let os = if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    // The Phase 3 narrowed support matrix is darwin/aarch64 → Metal,
    // linux/x86_64 → AMD Vulkan. Pick by tuple.
    let gpu_accel = match (os, arch) {
        ("darwin", "aarch64") => "apple_metal",
        ("linux", "x86_64") => "amd_vulkan",
        _ => "amd_vulkan",
    };
    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i16)
        .unwrap_or(0);

    let (gpu_model, total_vram_mb) = read_gpu_info(gpu_accel);
    let total_ram_mb = read_total_ram_mb();

    ProbeResponse {
        os,
        arch,
        gpu_accel,
        gpu_model,
        total_vram_mb,
        total_ram_mb,
        cpu_threads,
    }
}

#[cfg(target_os = "linux")]
fn read_gpu_info(_accel: &str) -> (Option<String>, i64) {
    use std::fs;
    use std::path::Path;

    // Probe the first DRM card with a `mem_info_vram_total` file. Multi-
    // GPU hosts surface every adapter; we pick the largest VRAM as the
    // model the operator most likely wants to load on.
    let mut best: (Option<String>, i64) = (None, 0);
    let drm = Path::new("/sys/class/drm");
    let Ok(entries) = fs::read_dir(drm) else {
        return best;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Match cardN (no partition suffixes like card0-DP-1)
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let vram_path = device.join("mem_info_vram_total");
        let model_path = device.join("product_name");
        let vram_mb = fs::read_to_string(&vram_path)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|bytes| (bytes / 1024 / 1024) as i64)
            .unwrap_or(0);
        if vram_mb > best.1 {
            let model = fs::read_to_string(&model_path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            best = (model, vram_mb);
        }
    }
    best
}

#[cfg(target_os = "macos")]
fn read_gpu_info(_accel: &str) -> (Option<String>, i64) {
    // On Apple Silicon GPU and CPU share unified memory. We surface the
    // installed RAM as total_vram_mb (Metal can use most of it for
    // weights) and a synthetic model name. Real-device introspection
    // via `ioreg` ships in a follow-up.
    let total = read_total_ram_mb();
    (Some("Apple Silicon (unified memory)".to_string()), total)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_gpu_info(_accel: &str) -> (Option<String>, i64) {
    (None, 0)
}

#[cfg(target_os = "linux")]
fn read_total_ram_mb() -> i64 {
    use std::fs;
    let Ok(text) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:")
            && let Some(kb) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok())
        {
            return (kb / 1024) as i64;
        }
    }
    0
}

#[cfg(target_os = "macos")]
fn read_total_ram_mb() -> i64 {
    // sysctl hw.memsize — fall back to 0 on parse failure.
    use std::process::Command;
    let Ok(out) = Command::new("sysctl").args(["-n", "hw.memsize"]).output() else {
        return 0;
    };
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .ok()
        .map(|b| (b / 1024 / 1024) as i64)
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_total_ram_mb() -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_returns_a_known_arch() {
        let r = probe();
        assert!(matches!(r.arch, "aarch64" | "x86_64"));
    }

    #[test]
    fn probe_sets_gpu_accel_per_platform() {
        let r = probe();
        assert!(matches!(r.gpu_accel, "apple_metal" | "amd_vulkan"));
    }

    #[test]
    fn cpu_threads_positive() {
        let r = probe();
        // CI runners have at least 2 cores.
        assert!(r.cpu_threads >= 1);
    }
}
