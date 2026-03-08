use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use super::state::AppState;

// ── Prometheus HTTP Service Discovery ─────────────────────────────────────────
//
// OTel Collector (prometheus receiver) polls this endpoint every 30 s to
// discover which node-exporter instances to scrape.
//
// Format: https://prometheus.io/docs/prometheus/latest/http_sd/
//
// OTel config:
//   receivers:
//     prometheus:
//       config:
//         scrape_configs:
//           - job_name: node-exporter
//             http_sd_configs:
//               - url: http://veronex:3000/v1/metrics/targets
//                 refresh_interval: 30s

#[derive(Serialize)]
struct SdTarget {
    targets: Vec<String>,
    labels: std::collections::HashMap<String, String>,
}

/// `GET /v1/metrics/targets`
///
/// Returns registered node-exporter endpoints in Prometheus HTTP Service
/// Discovery format.  One target per GPU server (deduplicates providers sharing
/// the same physical host).  Only servers with `node_exporter_url` set are
/// included.
pub async fn list_metrics_targets(State(state): State<AppState>) -> impl IntoResponse {
    let servers = match state.gpu_server_registry.list_all().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("metrics targets: failed to list gpu servers: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }
    };

    let targets: Vec<SdTarget> = servers
        .into_iter()
        .filter_map(|s| {
            let ne_url = s.node_exporter_url.filter(|u| !u.is_empty())?;

            // Strip scheme — Prometheus SD wants "host:port" not "http://host:port".
            let target = ne_url
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .to_string();

            // Extract bare hostname from the node-exporter URL for the label.
            // e.g. "http://192.168.1.10:9100" → "192.168.1.10"
            let host = ne_url
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .split(':')
                .next()
                .unwrap_or("")
                .to_string();

            let mut labels = std::collections::HashMap::new();
            labels.insert("server_id".to_string(), s.id.to_string());
            labels.insert("server_name".to_string(), s.name.clone());
            labels.insert("host".to_string(), host);

            Some(SdTarget {
                targets: vec![target],
                labels,
            })
        })
        .collect();

    (StatusCode::OK, Json(targets)).into_response()
}
