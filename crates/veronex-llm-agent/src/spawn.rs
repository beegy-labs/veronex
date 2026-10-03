//! `/spawn` — launch a `llama-server` child process.
//!
//! Inputs map straight onto the llama-server CLI:
//!
//! - `--model` ← `blob_path` (resolved by `blob_pv`)
//! - `--port`  ← caller-allocated port
//! - `--n-gpu-layers`, `--ctx-size`, `--parallel` ← Modelfile.runtime
//!
//! Phase 3 ships the request/response shape and a stub spawn that
//! returns 501 unless the binary path is configured. The Mac-specific
//! `DYLD_LIBRARY_PATH` and the Linux-specific Vulkan ICD env arrive
//! together with `mac/` and `linux/` sub-modules in a follow-up commit.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnRequest {
    /// `family:quantization` — opaque to the agent; logged for traces.
    pub model_id: String,
    /// sha256 of the GGUF blob. The agent resolves to a local path via
    /// the blob PV; if missing it 409s with a "fetch first" hint so
    /// the caller can `POST /blobs/{sha256}` then retry.
    pub blob_sha256: String,
    /// Port from the API server's PortPool — agent does NOT pick.
    pub port: u16,
    /// Subset of llama-server CLI options the API server is allowed to
    /// pass. Unknown keys are ignored for forward compatibility.
    #[serde(default)]
    pub runtime: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpawnResponse {
    pub pid: u32,
    /// Stable identifier so subsequent `DELETE /process/{pid}` and
    /// `GET /health/{port}` calls can correlate; the agent keeps it
    /// alongside the OS pid in its in-memory registry.
    pub agent_handle: Uuid,
    pub port: u16,
}

/// Build the llama-server arg vector from a runtime JSON blob. Pure —
/// the actual `Command::spawn` lives in the platform sub-modules.
pub fn build_args(req: &SpawnRequest, blob_path: &std::path::Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--model".into(),
        blob_path.to_string_lossy().into_owned(),
        "--port".into(),
        req.port.to_string(),
        "--host".into(),
        "0.0.0.0".into(),
    ];

    // n-gpu-layers — default to all (-1) which llama-server interprets
    // as "load every layer onto the GPU". Operators override per
    // Modelfile (e.g. partial offload on small VRAM).
    let n_gpu_layers = req
        .runtime
        .get("n_gpu_layers")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    args.push("--n-gpu-layers".into());
    args.push(n_gpu_layers.to_string());

    if let Some(ctx) = req.runtime.get("ctx_size").and_then(|v| v.as_u64()) {
        args.push("--ctx-size".into());
        args.push(ctx.to_string());
    }
    if let Some(parallel) = req.runtime.get("parallel").and_then(|v| v.as_u64()) {
        args.push("--parallel".into());
        args.push(parallel.to_string());
    }
    if let Some(threads) = req.runtime.get("threads").and_then(|v| v.as_u64()) {
        args.push("--threads".into());
        args.push(threads.to_string());
    }
    // `--metrics` exposes slots_idle/slots_processing on /health which
    // Phase 1 routing relies on. Emit it by default.
    args.push("--metrics".into());

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn req_with(runtime: serde_json::Value) -> SpawnRequest {
        SpawnRequest {
            model_id: "test:q4".into(),
            blob_sha256: "abc".into(),
            port: 11431,
            runtime,
        }
    }

    #[test]
    fn build_args_includes_required_flags() {
        let r = req_with(serde_json::json!({}));
        let args = build_args(&r, &PathBuf::from("/blobs/abc.gguf"));
        let joined = args.join(" ");
        assert!(joined.contains("--model /blobs/abc.gguf"));
        assert!(joined.contains("--port 11431"));
        assert!(joined.contains("--host 0.0.0.0"));
        assert!(joined.contains("--metrics"));
        // n_gpu_layers defaults to -1.
        assert!(joined.contains("--n-gpu-layers -1"));
    }

    #[test]
    fn runtime_overrides_propagate() {
        let r = req_with(serde_json::json!({
            "n_gpu_layers": 32,
            "ctx_size": 8192,
            "parallel": 8,
            "threads": 16,
        }));
        let args = build_args(&r, &PathBuf::from("/blobs/abc.gguf"));
        let joined = args.join(" ");
        assert!(joined.contains("--n-gpu-layers 32"));
        assert!(joined.contains("--ctx-size 8192"));
        assert!(joined.contains("--parallel 8"));
        assert!(joined.contains("--threads 16"));
    }

    #[test]
    fn unknown_runtime_keys_ignored() {
        let r = req_with(serde_json::json!({ "future_flag": "x" }));
        let args = build_args(&r, &PathBuf::from("/blobs/abc.gguf"));
        // Just make sure we don't blow up; future_flag is silently dropped.
        assert!(args.iter().any(|a| a == "--metrics"));
    }
}
