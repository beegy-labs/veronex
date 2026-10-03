//! Veronex-side client for `veronex-llm-agent` (Phase 3).
//!
//! Two implementations:
//!
//! - [`HttpNodeClient`] — production. Talks to a remote agent over
//!   HTTP using the contract documented in
//!   `crates/veronex-llm-agent/src/api.rs`.
//! - [`StubNodeClient`] — pre-deployment / test fixture. Returns
//!   deterministic canned values so the manager and admin handlers
//!   can be exercised without a real agent.
//!
//! The orchestrator (`manager.rs`, follow-up commit) holds one
//! `Arc<dyn NodeClient>` per registered `LlmNode`.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entities::{GpuAccel, HostArch, HostOs, NodeProbeInfo};

/// Spawn-call payload mirroring the agent's `SpawnRequest`.
#[derive(Debug, Clone, Serialize)]
pub struct SpawnRequest {
    pub model_id: String,
    pub blob_sha256: String,
    pub port: u16,
    /// Forwarded verbatim — the agent ignores unknown keys.
    pub runtime: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnResponse {
    pub pid: u32,
    pub agent_handle: Uuid,
    pub port: u16,
}

#[async_trait]
pub trait NodeClient: Send + Sync {
    /// Hit the agent's `/probe` and return the resolved hardware info.
    async fn probe(&self) -> Result<NodeProbeInfo>;

    /// Ask the agent to spawn a llama-server. Returns the agent-side
    /// handle and pid; both are ignored by the manager when the agent
    /// is in stub mode.
    async fn spawn(&self, req: &SpawnRequest) -> Result<SpawnResponse>;

    /// Ask the agent to stop a previously-spawned process. Idempotent.
    async fn stop(&self, agent_handle: Uuid) -> Result<()>;

    /// Trigger a streaming blob fetch from Garage to the agent's local
    /// PV. The agent verifies sha256 and stores under
    /// `{base}/blobs/{sha256}.gguf`.
    async fn ensure_blob(&self, sha256: &str, source_url: &str) -> Result<()>;
}

// ── HTTP impl ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct HttpNodeClient {
    base_url: String,
    client: reqwest::Client,
}

impl HttpNodeClient {
    pub fn new(base_url: impl Into<String>, client: reqwest::Client) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

/// Helper — converts the agent's `/probe` JSON into the domain type.
fn parse_probe_response(v: serde_json::Value) -> Result<NodeProbeInfo> {
    let os: String = v
        .get("os")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("/probe missing os"))?
        .to_string();
    let arch: String = v
        .get("arch")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("/probe missing arch"))?
        .to_string();
    let gpu_accel: String = v
        .get("gpu_accel")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("/probe missing gpu_accel"))?
        .to_string();
    let gpu_model = v
        .get("gpu_model")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let total_vram_mb = v
        .get("total_vram_mb")
        .and_then(|n| n.as_i64())
        .unwrap_or(0);
    let total_ram_mb = v
        .get("total_ram_mb")
        .and_then(|n| n.as_i64())
        .unwrap_or(0);
    let cpu_threads = v
        .get("cpu_threads")
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i16;

    Ok(NodeProbeInfo {
        os: os.parse::<HostOs>().map_err(|e| anyhow!(e))?,
        arch: arch.parse::<HostArch>().map_err(|e| anyhow!(e))?,
        gpu_accel: gpu_accel.parse::<GpuAccel>().map_err(|e| anyhow!(e))?,
        gpu_model,
        total_vram_mb,
        total_ram_mb,
        cpu_threads,
    })
}

#[async_trait]
impl NodeClient for HttpNodeClient {
    async fn probe(&self) -> Result<NodeProbeInfo> {
        let resp = self
            .client
            .get(self.url("/probe"))
            .send()
            .await
            .context("agent /probe")?
            .error_for_status()
            .context("agent /probe non-2xx")?;
        let v: serde_json::Value = resp.json().await.context("agent /probe JSON")?;
        parse_probe_response(v)
    }

    async fn spawn(&self, req: &SpawnRequest) -> Result<SpawnResponse> {
        let resp = self
            .client
            .post(self.url("/spawn"))
            .json(req)
            .send()
            .await
            .context("agent /spawn")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("agent /spawn returned {status}: {body}"));
        }
        resp.json::<SpawnResponse>()
            .await
            .context("agent /spawn JSON")
    }

    async fn stop(&self, agent_handle: Uuid) -> Result<()> {
        let resp = self
            .client
            .delete(self.url(&format!("/process/{agent_handle}")))
            .send()
            .await
            .context("agent /process DELETE")?;
        // 404 is fine — the process is already gone (idempotent).
        if resp.status().as_u16() == 404 {
            return Ok(());
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("agent /process DELETE returned {status}: {body}"));
        }
        Ok(())
    }

    async fn ensure_blob(&self, sha256: &str, source_url: &str) -> Result<()> {
        let resp = self
            .client
            .post(self.url(&format!("/blobs/{sha256}")))
            .json(&serde_json::json!({ "url": source_url }))
            .send()
            .await
            .context("agent /blobs POST")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("agent /blobs POST returned {status}: {body}"));
        }
        Ok(())
    }
}

// ── Stub impl ────────────────────────────────────────────────────────────

/// Test fixture. Returns deterministic responses for every method.
/// Used by integration tests that exercise ProcessManager + admin
/// endpoints without a real agent network.
#[derive(Clone, Default)]
pub struct StubNodeClient {
    pub probe_info: Option<NodeProbeInfo>,
}

#[async_trait]
impl NodeClient for StubNodeClient {
    async fn probe(&self) -> Result<NodeProbeInfo> {
        Ok(self.probe_info.clone().unwrap_or(NodeProbeInfo {
            os: HostOs::Linux,
            arch: HostArch::X86_64,
            gpu_accel: GpuAccel::AmdVulkan,
            gpu_model: Some("Stub GPU".into()),
            total_vram_mb: 32768,
            total_ram_mb: 65536,
            cpu_threads: 16,
        }))
    }

    async fn spawn(&self, req: &SpawnRequest) -> Result<SpawnResponse> {
        Ok(SpawnResponse {
            pid: 1,
            agent_handle: Uuid::now_v7(),
            port: req.port,
        })
    }

    async fn stop(&self, _agent_handle: Uuid) -> Result<()> {
        Ok(())
    }

    async fn ensure_blob(&self, _sha256: &str, _source_url: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_probe_response_full() {
        let v = serde_json::json!({
            "os": "linux",
            "arch": "x86_64",
            "gpu_accel": "amd_vulkan",
            "gpu_model": "AMD Radeon",
            "total_vram_mb": 16384,
            "total_ram_mb": 65536,
            "cpu_threads": 24,
        });
        let p = parse_probe_response(v).unwrap();
        assert_eq!(p.os, HostOs::Linux);
        assert_eq!(p.arch, HostArch::X86_64);
        assert_eq!(p.gpu_accel, GpuAccel::AmdVulkan);
        assert_eq!(p.gpu_model.as_deref(), Some("AMD Radeon"));
        assert_eq!(p.total_vram_mb, 16384);
        assert_eq!(p.cpu_threads, 24);
    }

    #[test]
    fn parse_probe_response_missing_os_fails() {
        let v = serde_json::json!({ "arch": "x86_64" });
        assert!(parse_probe_response(v).is_err());
    }

    #[tokio::test]
    async fn stub_probe_returns_default() {
        let c = StubNodeClient::default();
        let p = c.probe().await.unwrap();
        assert_eq!(p.gpu_accel, GpuAccel::AmdVulkan);
    }

    #[tokio::test]
    async fn stub_spawn_echoes_port() {
        let c = StubNodeClient::default();
        let r = c
            .spawn(&SpawnRequest {
                model_id: "x:q".into(),
                blob_sha256: "y".into(),
                port: 11430,
                runtime: serde_json::json!({}),
            })
            .await
            .unwrap();
        assert_eq!(r.port, 11430);
    }
}
