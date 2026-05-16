//! Phase 3 — managed compute hosts that can run `llama-server`.
//!
//! Two deployment kinds and a small fixed set of `gpu_accel` values: Mac
//! M-chip (Apple Metal, baremetal) and AMD Strix Halo (Vulkan, k8s
//! DaemonSet). NVIDIA/ROCm/Intel are explicitly out of scope per the
//! 2026-05-09 narrowing decision.
//!
//! The node sits behind an `agent_url` (HTTP). The ProcessManager talks to
//! the agent for `spawn` / `stop` / `health` / `probe` / blob staging.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentKind {
    /// Pod scheduled by k8s; auto-registers on agent boot.
    K8s,
    /// Mac mini / Studio outside the cluster, registered manually.
    BaremetalMac,
}

impl DeploymentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::K8s => "k8s",
            Self::BaremetalMac => "baremetal_mac",
        }
    }
}

impl std::str::FromStr for DeploymentKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "k8s" => Ok(Self::K8s),
            "baremetal_mac" => Ok(Self::BaremetalMac),
            other => Err(format!("unknown deployment_kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostOs {
    Linux,
    Darwin,
}

impl HostOs {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Darwin => "darwin",
        }
    }
}

impl std::str::FromStr for HostOs {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linux" => Ok(Self::Linux),
            "darwin" => Ok(Self::Darwin),
            other => Err(format!("unknown os: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostArch {
    /// AMD64 (Strix Halo k8s nodes).
    X86_64,
    /// ARM64 (Apple Silicon).
    Aarch64,
}

impl HostArch {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

impl std::str::FromStr for HostArch {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "x86_64" | "amd64" => Ok(Self::X86_64),
            "aarch64" | "arm64" => Ok(Self::Aarch64),
            other => Err(format!("unknown arch: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuAccel {
    /// Apple Metal (Mac M-chip).
    AppleMetal,
    /// AMD Vulkan via Mesa / radv (Strix Halo iGPU).
    AmdVulkan,
}

impl GpuAccel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AppleMetal => "apple_metal",
            Self::AmdVulkan => "amd_vulkan",
        }
    }
}

impl std::str::FromStr for GpuAccel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "apple_metal" => Ok(Self::AppleMetal),
            "amd_vulkan" => Ok(Self::AmdVulkan),
            other => Err(format!("unknown gpu_accel: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmNode {
    pub id: Uuid,
    pub hostname: String,
    pub deployment_kind: DeploymentKind,
    pub os: HostOs,
    pub arch: HostArch,
    pub gpu_accel: GpuAccel,
    pub gpu_model: Option<String>,
    pub total_vram_mb: i64,
    pub total_ram_mb: i64,
    pub cpu_threads: i16,
    pub agent_url: String,
    pub status: String,
    pub registered_at: DateTime<Utc>,
    pub last_probe_at: Option<DateTime<Utc>>,
}

/// Probe response from the agent — drives `last_probe_at` updates and
/// auto-fills hardware fields when an operator first registers a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProbeInfo {
    pub os: HostOs,
    pub arch: HostArch,
    pub gpu_accel: GpuAccel,
    pub gpu_model: Option<String>,
    pub total_vram_mb: i64,
    pub total_ram_mb: i64,
    pub cpu_threads: i16,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    #[test]
    fn enum_round_trips() {
        for v in [DeploymentKind::K8s, DeploymentKind::BaremetalMac] {
            assert_eq!(DeploymentKind::from_str(v.as_str()).unwrap(), v);
        }
        for v in [HostOs::Linux, HostOs::Darwin] {
            assert_eq!(HostOs::from_str(v.as_str()).unwrap(), v);
        }
        for v in [HostArch::X86_64, HostArch::Aarch64] {
            assert_eq!(HostArch::from_str(v.as_str()).unwrap(), v);
        }
        for v in [GpuAccel::AppleMetal, GpuAccel::AmdVulkan] {
            assert_eq!(GpuAccel::from_str(v.as_str()).unwrap(), v);
        }
    }

    #[test]
    fn arch_accepts_aliases() {
        assert_eq!(HostArch::from_str("amd64").unwrap(), HostArch::X86_64);
        assert_eq!(HostArch::from_str("arm64").unwrap(), HostArch::Aarch64);
    }
}
