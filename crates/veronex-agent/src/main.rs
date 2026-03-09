/// veronex-agent: Scrapes node-exporter + Ollama metrics from registered
/// GPU servers and pushes them to OTel Collector via OTLP HTTP.
///
/// Environment variables:
///   VERONEX_API_URL   — veronex API base URL (default: http://localhost:3000)
///   OTEL_HTTP_ENDPOINT — OTel Collector HTTP endpoint (default: http://localhost:4318)
///   SCRAPE_INTERVAL    — seconds between scrape cycles (default: 30)
///   RUST_LOG           — tracing filter (default: info)
use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

mod otlp;
mod scraper;

// ── Configuration ────────────────────────────────────────────────────────────

struct Config {
    veronex_api_url: String,
    otel_endpoint: String,
    scrape_interval: Duration,
}

impl Config {
    fn from_env() -> Self {
        Self {
            veronex_api_url: std::env::var("VERONEX_API_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            otel_endpoint: std::env::var("OTEL_HTTP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4318".into()),
            scrape_interval: Duration::from_secs(
                std::env::var("SCRAPE_INTERVAL")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30),
            ),
        }
    }
}

// ── Target discovery (from veronex API) ──────────────────────────────────────

/// A scrape target discovered from veronex `/v1/metrics/targets`.
#[derive(Debug, Clone, Deserialize)]
struct SdTarget {
    targets: Vec<String>,
    labels: HashMap<String, String>,
}

/// Fetch registered GPU server targets from veronex API.
async fn discover_targets(client: &reqwest::Client, api_url: &str) -> Vec<SdTarget> {
    let url = format!("{}/v1/metrics/targets", api_url.trim_end_matches('/'));
    match client.get(&url).timeout(Duration::from_secs(10)).send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<Vec<SdTarget>>().await.unwrap_or_default()
        }
        Ok(resp) => {
            tracing::warn!(status = %resp.status(), "failed to fetch targets from veronex");
            vec![]
        }
        Err(e) => {
            tracing::warn!("target discovery failed: {e}");
            vec![]
        }
    }
}

// ── Main loop ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::from_env();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    tracing::info!(
        api = %config.veronex_api_url,
        otel = %config.otel_endpoint,
        interval = config.scrape_interval.as_secs(),
        "veronex-agent started"
    );

    loop {
        let targets = discover_targets(&client, &config.veronex_api_url).await;

        if targets.is_empty() {
            tracing::debug!("no targets discovered, sleeping");
        }

        for target in &targets {
            let Some(host_port) = target.targets.first() else {
                continue;
            };
            let server_id = target.labels.get("server_id").cloned().unwrap_or_default();
            let server_name = target.labels.get("server_name").cloned().unwrap_or_default();

            // node-exporter URL: targets are "host:port" without scheme
            let ne_url = format!("http://{host_port}");

            // Derive Ollama URL from node-exporter host (same host, port 11434)
            let ollama_host = host_port.split(':').next().unwrap_or(host_port);
            let ollama_url = format!("http://{ollama_host}:11434");

            // Scrape and push
            let metrics = scraper::scrape(&client, &ne_url, &ollama_url).await;
            if let Err(e) = otlp::push_metrics(
                &client,
                &config.otel_endpoint,
                &server_id,
                &server_name,
                &metrics,
            )
            .await
            {
                tracing::warn!(
                    server = %server_name,
                    "OTLP push failed: {e}"
                );
            } else {
                tracing::debug!(
                    server = %server_name,
                    gauge_count = metrics.len(),
                    "pushed metrics"
                );
            }
        }

        tokio::time::sleep(config.scrape_interval).await;
    }
}
