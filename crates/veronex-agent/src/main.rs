/// veronex-agent: Collects hardware + Ollama metrics independently and pushes
/// them to OTel Collector via OTLP HTTP.
///
/// Two target types, each collected on its own:
///   type=server  — node-exporter (CPU, mem, GPU)
///   type=ollama  — Ollama /api/ps (loaded models, VRAM)
///
/// When linked (server_id FK), analytics can correlate both.
///
/// Supports N replicas via modulus sharding — no external coordination.
///
/// Environment variables:
///   VERONEX_API_URL    — veronex API base URL (default: http://localhost:3000)
///   OTEL_HTTP_ENDPOINT — OTel Collector HTTP endpoint (default: http://localhost:4318)
///   SCRAPE_INTERVAL_MS — milliseconds between scrape cycles (default: 15000)
///   REPLICA_COUNT      — total number of agent pods (default: 1)
///   RUST_LOG           — tracing filter (default: info)
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use tokio::signal;
use tokio::sync::Semaphore;

mod otlp;
mod scraper;
mod shard;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Max concurrent scrape tasks to prevent resource exhaustion.
const MAX_CONCURRENT_SCRAPES: usize = 32;

// ── Configuration ────────────────────────────────────────────────────────────

struct Config {
    veronex_api_url: String,
    otel_endpoint: String,
    scrape_interval: Duration,
    ordinal: u32,
    replicas: u32,
}

impl Config {
    fn from_env() -> Self {
        Self {
            veronex_api_url: std::env::var("VERONEX_API_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            otel_endpoint: std::env::var("OTEL_HTTP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4318".into()),
            scrape_interval: Duration::from_millis(
                std::env::var("SCRAPE_INTERVAL_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(15_000),
            ),
            ordinal: shard::ordinal_from_hostname(),
            replicas: std::env::var("REPLICA_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
        }
    }
}

// ── Target discovery ─────────────────────────────────────────────────────────

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
            tracing::debug!(status = %resp.status(), "target discovery returned non-success");
            vec![]
        }
        Err(e) => {
            tracing::debug!("target discovery failed: {e}");
            vec![]
        }
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

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
    let scrape_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_SCRAPES));

    tracing::info!(
        api = %config.veronex_api_url,
        otel = %config.otel_endpoint,
        ordinal = config.ordinal,
        replicas = config.replicas,
        interval_ms = config.scrape_interval.as_millis() as u64,
        "veronex-agent started"
    );

    loop {
        tokio::select! {
            biased;
            _ = signal::ctrl_c() => {
                tracing::info!("shutdown signal received");
                break;
            }
            _ = scrape_cycle(&client, &config, &scrape_semaphore) => {}
        }
        tokio::time::sleep(config.scrape_interval).await;
    }

    tracing::info!("veronex-agent stopped");
    Ok(())
}

/// Shard key for a target — server_id for servers, provider_id for ollama.
fn shard_key(t: &SdTarget) -> &str {
    match t.labels.get("type").map(|s| s.as_str()) {
        Some("server") => t.labels.get("server_id").map(|s| s.as_str()).unwrap_or(""),
        Some("ollama") => t.labels.get("provider_id").map(|s| s.as_str()).unwrap_or(""),
        _ => "",
    }
}

async fn scrape_cycle(client: &reqwest::Client, config: &Config, semaphore: &Arc<Semaphore>) {
    let targets = discover_targets(client, &config.veronex_api_url).await;
    if targets.is_empty() {
        tracing::debug!("no targets discovered");
        return;
    }

    let my_targets: Vec<_> = targets
        .iter()
        .filter(|t| shard::owns(shard_key(t), config.ordinal, config.replicas))
        .collect();

    if my_targets.is_empty() {
        return;
    }

    let futures: Vec<_> = my_targets
        .iter()
        .filter_map(|t| {
            let host_port = t.targets.first()?;
            let target_type = t.labels.get("type")?.as_str();
            let url = format!("http://{host_port}");
            let labels = t.labels.clone();
            let sem = semaphore.clone();

            Some(async move {
                let _permit = sem.acquire().await;
                match target_type {
                    "server" => {
                        let metrics = scraper::scrape_node_exporter(client, &url).await;
                        (labels, metrics)
                    }
                    "ollama" => {
                        let metrics = scraper::scrape_ollama(client, &url).await;
                        (labels, metrics)
                    }
                    _ => (labels, vec![]),
                }
            })
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    for (labels, metrics) in &results {
        if metrics.is_empty() {
            continue;
        }
        if let Err(e) = otlp::push_metrics(client, &config.otel_endpoint, labels, metrics).await {
            tracing::warn!(target = ?labels.get("type"), "OTLP push failed: {e}");
        }
    }
}
