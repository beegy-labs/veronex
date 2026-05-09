//! HTTP API surface — the Veronex side talks to the agent here.
//!
//! Phase 3 minimal implementation:
//! - `/probe`           — real (system info)
//! - `/healthz`         — real
//! - `/health/{port}`   — proxies llama-server `/health`
//! - `/blobs/{sha256}`  — GET = presence (real); POST = 501 until the
//!                        streaming fetch lands
//! - `/spawn`           — 501 until platform-specific spawn lands
//! - `/process/{handle}` — DELETE = 501; the registry returns 404 when
//!                         the handle is unknown

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::blob_pv::BlobPv;
use crate::process::ProcessRegistry;
use crate::spawn::SpawnRequest;
use crate::probe;

#[derive(Clone)]
pub struct AgentState {
    pub blob_pv: BlobPv,
    pub registry: ProcessRegistry,
    /// Path to the `llama-server` binary — `None` when the agent runs
    /// in stub mode for tests / pre-deployment validation.
    pub binary_path: Option<Arc<std::path::PathBuf>>,
    pub http_client: reqwest::Client,
}

impl AgentState {
    pub fn new(blob_pv: BlobPv) -> Self {
        Self {
            blob_pv,
            registry: ProcessRegistry::new(),
            binary_path: None,
            http_client: reqwest::Client::new(),
        }
    }
}

pub fn router(state: AgentState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/probe", get(probe_handler))
        .route("/blobs/{sha256}", get(blob_status).post(blob_fetch))
        .route("/spawn", post(spawn_process))
        .route("/process/{handle}", delete(stop_process))
        .route("/process", get(list_processes))
        .route("/health/{port}", get(proxy_health))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn probe_handler() -> impl IntoResponse {
    Json(probe::probe())
}

async fn blob_status(
    State(state): State<AgentState>,
    Path(sha256): Path<String>,
) -> impl IntoResponse {
    Json(state.blob_pv.status(&sha256).await)
}

#[derive(Debug, Deserialize)]
struct BlobFetchRequest {
    /// Pre-signed URL or direct S3 URL the agent should download from.
    /// The full S3 client config (endpoint + creds) lives on the API
    /// server, so the simplest contract is a URL the agent fetches.
    #[allow(dead_code)]
    url: String,
}

async fn blob_fetch(
    State(_state): State<AgentState>,
    Path(_sha256): Path<String>,
    Json(_req): Json<BlobFetchRequest>,
) -> impl IntoResponse {
    // The streaming fetch + sha256 verify path lands with the API
    // server's agent_client implementation; until then a clear 501
    // beats a confusing partial-fetch failure.
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "blob_fetch_not_implemented",
            "message": "Streaming blob fetch lands together with agent_client wiring on the API side.",
        })),
    )
}

async fn spawn_process(
    State(state): State<AgentState>,
    Json(req): Json<SpawnRequest>,
) -> impl IntoResponse {
    if state.binary_path.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "llama_server_binary_missing",
                "message": "Agent has no llama-server binary configured. Install via homebrew (mac) or in the agent image (k8s).",
            })),
        )
            .into_response();
    }
    // Even with a binary configured, real spawn arrives in the
    // platform sub-modules (mac/spawn.rs, linux/spawn.rs). Until then
    // we emit 501 so the API side can surface a clear status.
    let _ = req; // keep the field mapping stable for compile.
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "spawn_not_implemented",
            "message": "Platform-specific spawn (Metal / Vulkan ICD) ships with the agent integration commit.",
        })),
    )
        .into_response()
}

async fn stop_process(
    State(state): State<AgentState>,
    Path(handle): Path<Uuid>,
) -> impl IntoResponse {
    match state.registry.get(handle) {
        Some(_) => (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "stop_not_implemented",
                "message": "SIGTERM ladder ships with platform spawn support.",
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "process_not_found", "handle": handle })),
        )
            .into_response(),
    }
}

async fn list_processes(State(state): State<AgentState>) -> impl IntoResponse {
    Json(json!({ "processes": state.registry.list() }))
}

async fn proxy_health(
    State(state): State<AgentState>,
    Path(port): Path<u16>,
) -> impl IntoResponse {
    let url = format!("http://127.0.0.1:{port}/health");
    match state.http_client.get(&url).send().await {
        Ok(resp) => match resp.bytes().await {
            Ok(body) => {
                let parsed: serde_json::Value =
                    serde_json::from_slice(&body).unwrap_or(json!({}));
                (StatusCode::OK, Json(parsed)).into_response()
            }
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "upstream_body_error", "message": e.to_string() })),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "upstream_unreachable", "message": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn router_builds_without_panic() {
        let pv = BlobPv::new(std::env::temp_dir().join("vlm-agent-router-test"));
        pv.ensure_root().await.unwrap();
        let _r = router(AgentState::new(pv));
    }
}
