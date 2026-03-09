/// veronex-agent: Scrapes node-exporter + Ollama metrics from registered
/// GPU servers and pushes them to OTel Collector via OTLP HTTP.
///
/// Environment variables:
///   VERONEX_API_URL    — veronex API base URL (default: http://localhost:3000)
///   OTEL_HTTP_ENDPOINT — OTel Collector HTTP endpoint (default: http://localhost:4318)
///   SCRAPE_INTERVAL    — seconds between scrape cycles (default: 30)
///   RUST_LOG           — tracing filter (default: info)
use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use tokio::signal;

mod otlp;
mod scraper;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

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

#[derive(Debug, Clone, Deserialize)]
struct SdTarget {
    targets: Vec<String>,
    labels: HashMap<String, String>,
}

async fn discover_targets(client: &reqwest::Client, api_url: &str) -> Vec<SdTarget> {
    let url = format!("{}/v1/metrics/targets", api_url.trim_end_matches('/'));
    match client.get(&url).timeout(DISCOVERY_TIMEOUT).send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<Vec<SdTarget>>().await.unwrap_or_default()
        }
        Ok(resp) => {
            tracing::warn!(status = %resp.status(), "failed to fetch targets");
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
        tokio::select! {
            _ = signal::ctrl_c() => {
                tracing::info!("shutdown signal received");
                break;
            }
            _ = scrape_cycle(&client, &config) => {}
            _ = tokio::time::sleep(config.scrape_interval) => {}
        }
    }

    tracing::info!("veronex-agent stopped");
    Ok(())
}

async fn scrape_cycle(client: &reqwest::Client, config: &Config) {
    let targets = discover_targets(client, &config.veronex_api_url).await;
    if targets.is_empty() {
        tracing::debug!("no targets discovered");
        return;
    }

    // Scrape all targets concurrently
    let futures: Vec<_> = targets
        .iter()
        .filter_map(|t| {
            let host_port = t.targets.first()?;
            let server_id = t.labels.get("server_id").cloned().unwrap_or_default();
            let server_name = t.labels.get("server_name").cloned().unwrap_or_default();
            let ollama_url = t.labels.get("ollama_url").cloned();
            let ne_url = format!("http://{host_port}");

            Some(async move {
                let metrics =
                    scraper::scrape(client, &ne_url, ollama_url.as_deref()).await;
                (server_id, server_name, metrics)
            })
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    for (server_id, server_name, metrics) in results {
        if let Err(e) =
            otlp::push_metrics(client, &config.otel_endpoint, &server_id, &server_name, &metrics)
                .await
        {
            tracing::warn!(server = %server_name, "OTLP push failed: {e}");
        } else {
            tracing::debug!(server = %server_name, n = metrics.len(), "pushed");
        }
    }
}
