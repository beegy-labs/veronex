use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;

mod handlers;
mod otel;
mod state;

use handlers::auth_middleware;
use otel::OtlpClient;
use state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    // ── Tracing ────────────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // ── Config ─────────────────────────────────────────────────────────────────
    let clickhouse_url =
        std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
    let clickhouse_user =
        std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "veronex".to_string());
    let clickhouse_password =
        std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_else(|_| "veronex".to_string());
    let clickhouse_db =
        std::env::var("CLICKHOUSE_DB").unwrap_or_else(|_| "veronex".to_string());

    // OTLP HTTP endpoint (port 4318).  Default assumes OTel Collector on same host.
    let otel_http_endpoint = std::env::var("OTEL_HTTP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318".to_string());

    let analytics_secret = std::env::var("ANALYTICS_SECRET")
        .expect("ANALYTICS_SECRET env var is required — set a strong random token");

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3003);

    // ── ClickHouse ─────────────────────────────────────────────────────────────
    tracing::info!("connecting to clickhouse at {clickhouse_url}");
    let ch = clickhouse::Client::default()
        .with_url(&clickhouse_url)
        .with_user(&clickhouse_user)
        .with_password(&clickhouse_password)
        .with_database(&clickhouse_db);
    tracing::info!("clickhouse client ready");

    // ── OTLP HTTP client ───────────────────────────────────────────────────────
    let otlp = Arc::new(OtlpClient::new(&otel_http_endpoint));

    let state = AppState {
        ch,
        otlp,
        analytics_secret,
    };

    // ── Router ─────────────────────────────────────────────────────────────────
    let protected = Router::new()
        // Ingest
        .route("/internal/ingest/inference", post(handlers::ingest::ingest_inference))
        .route("/internal/ingest/audit", post(handlers::ingest::ingest_audit))
        // Usage
        .route("/internal/usage", get(handlers::usage::aggregate_usage))
        .route("/internal/usage/{key_id}", get(handlers::usage::key_usage))
        .route("/internal/usage/{key_id}/jobs", get(handlers::usage::key_usage_jobs))
        .route("/internal/analytics", get(handlers::usage::get_analytics))
        // Performance
        .route("/internal/performance", get(handlers::performance::get_performance))
        // Audit
        .route("/internal/audit", get(handlers::audit::list_audit_events))
        // Metrics history
        .route(
            "/internal/metrics/history/{server_id}",
            get(handlers::metrics::get_server_metrics_history),
        )
        // MCP stats
        .route("/internal/mcp/stats", get(handlers::mcp::get_mcp_stats))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(protected)
        .with_state(state);

    // ── Server ─────────────────────────────────────────────────────────────────
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("veronex-analytics listening on {addr}");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
