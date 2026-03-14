use std::collections::HashMap;

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
//   type=server  — node-exporter endpoints (hardware: CPU, mem, GPU)
//   type=ollama  — Ollama endpoints (loaded models, VRAM per model)
//
// When an Ollama provider is linked to a GpuServer (server_id FK), both
// targets carry matching server_id labels so analytics can correlate them.

#[derive(Serialize)]
struct SdTarget {
    targets: Vec<String>,
    labels: HashMap<String, String>,
}

/// Strip a URL down to `host[:port]` — removes scheme, path, query, fragment.
fn normalize_host_port(url: &str) -> String {
    let without_scheme = url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    // Strip path/query/fragment: take everything up to first '/' or '?'
    let host_port = without_scheme
        .split_once('/')
        .map(|(h, _)| h)
        .unwrap_or(without_scheme);
    let without_query = host_port
        .split_once('?')
        .map(|(h, _)| h)
        .unwrap_or(host_port);
    without_query
        .split_once('#')
        .map(|(h, _)| h)
        .unwrap_or(without_query)
        .to_string()
}

/// `GET /v1/metrics/targets`
///
/// Returns scrape targets for veronex-agent.  Server and Ollama targets are
/// returned independently — each is collected on its own, linked via
/// `server_id` when associated.
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
        let target = normalize_host_port(ne_url);

        let mut labels = HashMap::new();
        labels.insert("type".into(), "server".into());
        labels.insert("server_id".into(), s.id.to_string());
        labels.insert("server_name".into(), s.name.clone());

        targets.push(SdTarget { targets: vec![target], labels });
    }

    // ── Ollama targets ──────────────────────────────────────────────────
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
        if p.provider_type != ProviderType::Ollama || !p.is_active {
            continue;
        }
        let target = normalize_host_port(&p.url);

        let mut labels = HashMap::new();
        labels.insert("type".into(), "ollama".into());
        labels.insert("provider_id".into(), p.id.to_string());
        labels.insert("provider_name".into(), p.name.clone());

        // Link to server when associated
        if let Some(sid) = p.server_id {
            labels.insert("server_id".into(), sid.to_string());
        }

        targets.push(SdTarget { targets: vec![target], labels });
    }

    (StatusCode::OK, Json(targets)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Concrete example kept as documentation.
    #[test]
    fn normalize_strips_scheme_path_query_example() {
        assert_eq!(normalize_host_port("http://192.168.1.21:9100"), "192.168.1.21:9100");
        assert_eq!(normalize_host_port("192.168.1.21:9100"), "192.168.1.21:9100");
    }

    proptest! {
        #[test]
        fn normalize_always_strips_scheme_path_query_fragment(
            host in "[a-z]{3,10}",
            port in 1000u16..65535,
            path in prop::option::of("/[a-z]{0,5}"),
            query in prop::option::of("\\?[a-z]=[0-9]"),
            fragment in prop::option::of("#[a-z]+"),
        ) {
            let scheme = if port % 2 == 0 { "http" } else { "https" };
            let url = format!("{scheme}://{host}:{port}{}{}{}",
                path.as_deref().unwrap_or(""),
                query.as_deref().unwrap_or(""),
                fragment.as_deref().unwrap_or(""));
            let result = normalize_host_port(&url);
            prop_assert!(!result.starts_with("http"), "scheme not stripped: {result}");
            prop_assert!(!result.contains('/'), "path not stripped: {result}");
            prop_assert!(!result.contains('?'), "query not stripped: {result}");
            prop_assert!(!result.contains('#'), "fragment not stripped: {result}");
            prop_assert_eq!(result, format!("{host}:{port}"));
        }

        #[test]
        fn normalize_bare_host_port_is_identity(
            host in "[a-z]{3,10}",
            port in 1000u16..65535,
        ) {
            let input = format!("{host}:{port}");
            prop_assert_eq!(normalize_host_port(&input), input);
        }
    }
}
