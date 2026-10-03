use std::collections::HashMap;
use tracing::instrument;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::domain::enums::ProviderType;

use super::state::AppState;

// ── veronex-agent target discovery ───────────────────────────────────────────
//
// veronex-agent polls this endpoint each scrape cycle to discover scrape
// targets.  Two independent target types are returned:
//
//   type=server        — node-exporter endpoints (hardware: CPU, mem, GPU)
//   type=llama_server  — llama-server endpoints (loaded models, VRAM per model)
//
// When a llama_server provider is linked to a GpuServer (server_id FK), both
// targets carry matching server_id labels so analytics can correlate them.

#[derive(Serialize)]
struct SdTarget {
    targets: Vec<String>,
    labels: HashMap<String, String>,
}

/// `GET /v1/metrics/targets`
///
/// Returns scrape targets for veronex-agent.  Server and llama-server targets are
/// returned independently — each is collected on its own, linked via
/// `server_id` when associated.
#[instrument(skip_all)]
pub async fn list_metrics_targets(State(state): State<AppState>) -> impl IntoResponse {
    let mut targets: Vec<SdTarget> = Vec::new();

    // ── Server targets (node-exporter) ──────────────────────────────────
    let servers = match state.gpu_server_registry.list_all().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("metrics targets: failed to list gpu servers: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to list gpu servers"})),
            )
                .into_response();
        }
    };

    for s in &servers {
        let Some(ne_url) = s.node_exporter_url.as_deref().filter(|u| !u.is_empty()) else {
            continue;
        };
        let mut labels = HashMap::new();
        labels.insert("type".into(), "server".into());
        labels.insert("server_id".into(), s.id.to_string());
        labels.insert("server_name".into(), s.name.clone());

        targets.push(SdTarget { targets: vec![ne_url.to_string()], labels });
    }

    // ── llama-server targets ────────────────────────────────────────────
    let providers = match state.provider_registry.list_all().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("metrics targets: failed to list providers: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to list providers"})),
            )
                .into_response();
        }
    };

    for p in providers {
        if p.provider_type != ProviderType::LlamaServer {
            continue;
        }
        let mut labels = HashMap::new();
        labels.insert("type".into(), "llama_server".into());
        labels.insert("provider_id".into(), p.id.to_string());
        labels.insert("provider_name".into(), p.name.clone());
        labels.insert("total_vram_mb".into(), p.total_vram_mb.to_string());

        // Link to server when associated
        if let Some(sid) = p.server_id {
            labels.insert("server_id".into(), sid.to_string());
        }

        targets.push(SdTarget { targets: vec![p.url.clone()], labels });
    }

    (StatusCode::OK, Json(targets)).into_response()
}

