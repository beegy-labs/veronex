//! Phase 3 — admin endpoints for `llm_nodes`.
//!
//! Lifecycle is intentionally minimal: register a node, list/inspect,
//! delete. Hardware fields (`gpu_model`, `total_vram_mb`, etc.) are
//! filled in by a follow-up `POST /v1/admin/nodes/{id}/probe` call —
//! it issues `GET {agent_url}/probe` and rewrites the row. Until the
//! `veronex-agent` HTTP API is implemented, the probe handler returns
//! 501 so callers know to seed values manually at registration.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use tracing::instrument;

use crate::domain::entities::{
    DeploymentKind, GpuAccel, HostArch, HostOs, LlmNode,
};
use crate::infrastructure::inbound::http::middleware::jwt_auth::RequireProviderManage;
use crate::infrastructure::inbound::http::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterNodeRequest {
    pub hostname: String,
    pub deployment_kind: String,
    pub agent_url: String,
    /// Hardware fields are accepted optionally — set them at registration
    /// for nodes you've already inspected, or leave empty and call the
    /// probe endpoint after the agent is up.
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub gpu_accel: Option<String>,
    #[serde(default)]
    pub gpu_model: Option<String>,
    #[serde(default)]
    pub total_vram_mb: Option<i64>,
    #[serde(default)]
    pub total_ram_mb: Option<i64>,
    #[serde(default)]
    pub cpu_threads: Option<i16>,
}

fn not_wired() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "node_repo_not_wired",
            "message": "LlmNodeRepository is not configured. Phase 3 wiring is incomplete.",
        })),
    )
        .into_response()
}

fn bad_request(message: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "bad_request", "message": message })),
    )
        .into_response()
}

fn node_to_json(n: &LlmNode) -> serde_json::Value {
    json!({
        "id":              n.id,
        "hostname":        n.hostname,
        "deployment_kind": n.deployment_kind.as_str(),
        "os":              n.os.as_str(),
        "arch":            n.arch.as_str(),
        "gpu_accel":       n.gpu_accel.as_str(),
        "gpu_model":       n.gpu_model,
        "total_vram_mb":   n.total_vram_mb,
        "total_ram_mb":    n.total_ram_mb,
        "cpu_threads":     n.cpu_threads,
        "agent_url":       n.agent_url,
        "status":          n.status,
        "registered_at":   n.registered_at,
        "last_probe_at":   n.last_probe_at,
    })
}

/// `POST /v1/admin/nodes`
#[instrument(skip_all)]
pub async fn register_node(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Json(req): Json<RegisterNodeRequest>,
) -> impl IntoResponse {
    let Some(repo) = state.llm_node_repo.as_ref() else {
        return not_wired();
    };

    if req.hostname.trim().is_empty() {
        return bad_request("hostname is required");
    }
    if req.agent_url.trim().is_empty() {
        return bad_request("agent_url is required");
    }

    let deployment_kind = match req.deployment_kind.parse::<DeploymentKind>() {
        Ok(k) => k,
        Err(e) => return bad_request(&e),
    };

    // Defaults that match the Phase 3 narrowed support matrix:
    //   k8s             → linux/x86_64/amd_vulkan
    //   baremetal_mac   → darwin/aarch64/apple_metal
    let (default_os, default_arch, default_accel) = match deployment_kind {
        DeploymentKind::K8s => (HostOs::Linux, HostArch::X86_64, GpuAccel::AmdVulkan),
        DeploymentKind::BaremetalMac => {
            (HostOs::Darwin, HostArch::Aarch64, GpuAccel::AppleMetal)
        }
    };

    let os = match req.os {
        Some(s) => match s.parse::<HostOs>() {
            Ok(v) => v,
            Err(e) => return bad_request(&e),
        },
        None => default_os,
    };
    let arch = match req.arch {
        Some(s) => match s.parse::<HostArch>() {
            Ok(v) => v,
            Err(e) => return bad_request(&e),
        },
        None => default_arch,
    };
    let gpu_accel = match req.gpu_accel {
        Some(s) => match s.parse::<GpuAccel>() {
            Ok(v) => v,
            Err(e) => return bad_request(&e),
        },
        None => default_accel,
    };

    let node = LlmNode {
        id: Uuid::now_v7(),
        hostname: req.hostname.trim().to_string(),
        deployment_kind,
        os,
        arch,
        gpu_accel,
        gpu_model: req.gpu_model,
        total_vram_mb: req.total_vram_mb.unwrap_or(0),
        total_ram_mb: req.total_ram_mb.unwrap_or(0),
        cpu_threads: req.cpu_threads.unwrap_or(0),
        agent_url: req.agent_url.trim().to_string(),
        status: "pending_probe".to_string(),
        registered_at: chrono::Utc::now(),
        last_probe_at: None,
    };

    match repo.register(&node).await {
        Ok(_) => (StatusCode::CREATED, Json(node_to_json(&node))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "register_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `GET /v1/admin/nodes`
#[instrument(skip_all)]
pub async fn list_nodes(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(repo) = state.llm_node_repo.as_ref() else {
        return not_wired();
    };
    match repo.list_all().await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(node_to_json).collect();
            (StatusCode::OK, Json(json!({ "nodes": items, "total": items.len() })))
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "list_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `GET /v1/admin/nodes/{id}`
#[instrument(skip_all)]
pub async fn get_node(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(repo) = state.llm_node_repo.as_ref() else {
        return not_wired();
    };
    match repo.get(id).await {
        Ok(Some(n)) => (StatusCode::OK, Json(node_to_json(&n))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "node_not_found", "id": id })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "get_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `DELETE /v1/admin/nodes/{id}`
#[instrument(skip_all)]
pub async fn delete_node(
    _claims: RequireProviderManage,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(repo) = state.llm_node_repo.as_ref() else {
        return not_wired();
    };
    match repo.delete(id).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "node_not_found", "id": id })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "delete_failed", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// `POST /v1/admin/nodes/{id}/probe` — placeholder.
///
/// Final implementation calls `GET {agent_url}/probe` against the node's
/// agent and rewrites the hardware columns. Until the `veronex-agent`
/// HTTP API ships, the endpoint returns 501 so the operator knows to
/// seed values manually at registration.
#[instrument(skip_all)]
pub async fn probe_node(
    _claims: RequireProviderManage,
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "agent_probe_not_implemented",
            "message": "Agent /probe ships with veronex-agent; seed hardware fields at registration for now.",
        })),
    )
}
