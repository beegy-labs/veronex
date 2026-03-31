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
///   SCRAPE_INTERVAL_MS — milliseconds between scrape cycles (default: 60000)
///   REPLICA_COUNT      — total number of agent pods (default: 1)
///   HEALTH_PORT        — health probe HTTP port (default: 9091)
///   RUST_LOG           — tracing filter (default: info)
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Deserialize;
use tokio::signal;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

mod capacity_push;
mod health;
mod heartbeat;
mod mcp_discover;
mod orphan_sweeper;
mod otlp;
mod scraper;
mod shard;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Max concurrent scrape tasks to prevent resource exhaustion.
const MAX_CONCURRENT_SCRAPES: usize = 32;

/// Heartbeat TTL = 3× default scrape interval.  A provider survives 2 missed
/// cycles before being considered offline.
const HEARTBEAT_TTL_SECS: i64 = 180;

// ── Configuration ────────────────────────────────────────────────────────────

struct Config {
    veronex_api_url: String,
    otel_endpoint: String,
    scrape_interval: Duration,
    ordinal: u32,
    /// Static replica count from env (used as fallback when Valkey is unavailable).
    replicas_fallback: u32,
    health_port: u16,
    hostname: String,
    /// Optional Valkey URL for provider liveness heartbeats.
    /// When absent, heartbeat push is skipped (veronex falls back to HTTP probe).
    valkey_url: Option<String>,
    /// Optional DATABASE_URL for orphan job sweeper.
    /// When absent, orphan sweeper is disabled.
    database_url: Option<String>,
    /// veronex-embed URL for MCP tool embedding.
    /// When absent, tool embedding is skipped.
    embed_url: Option<String>,
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

impl Config {
    fn from_env() -> Self {
        Self {
            veronex_api_url: std::env::var("VERONEX_API_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            otel_endpoint: std::env::var("OTEL_HTTP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4318".into()),
            scrape_interval: Duration::from_millis(parse_env("SCRAPE_INTERVAL_MS", 60_000)),
            ordinal: shard::ordinal_from_hostname(),
            replicas_fallback: parse_env("REPLICA_COUNT", 1),
            health_port: parse_env("HEALTH_PORT", 9091),
            hostname: std::env::var("HOSTNAME").unwrap_or_else(|_| "veronex-agent-0".into()),
            valkey_url: std::env::var("VALKEY_URL").ok(),
            database_url: std::env::var("DATABASE_URL").ok(),
            embed_url: std::env::var("EMBED_URL").ok(),
        }
    }
}

// ── Health state ────────────────────────────────────────────────────────────

/// Shared health state between scrape loop and health HTTP server.
pub struct HealthState {
    pub started: AtomicBool,
    pub ready: AtomicBool,
    pub alive: AtomicBool,
}

impl HealthState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            started: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            alive: AtomicBool::new(true),
        })
    }
}

// ── Agent stats ─────────────────────────────────────────────────────────────

struct CycleResult {
    duration_secs: f64,
    targets_scraped: usize,
    gauges_collected: usize,
    success: bool,
}

struct AgentStats {
    scrape_errors: AtomicU64,
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
            match resp.json::<Vec<SdTarget>>().await {
                Ok(targets) => targets,
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "target discovery JSON parse failed");
                    vec![]
                }
            }
        }
        Ok(resp) => {
            tracing::warn!(url = %url, status = %resp.status(), "target discovery returned non-success");
            vec![]
        }
        Err(e) => {
            tracing::debug!(error = %e, "target discovery failed");
            vec![]
        }
    }
}

#[derive(Debug, Deserialize)]
struct McpTargetEntry {
    id: String,
    url: String,
}

/// Fetch enabled MCP servers from `/v1/mcp/targets` (no auth required).
async fn fetch_mcp_targets(client: &reqwest::Client, api_url: &str) -> Vec<(String, String)> {
    let url = format!("{}/v1/mcp/targets", api_url.trim_end_matches('/'));
    match client.get(&url).timeout(DISCOVERY_TIMEOUT).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<Vec<McpTargetEntry>>().await {
                Ok(entries) => entries.into_iter().map(|e| (e.id, e.url)).collect(),
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "MCP target discovery JSON parse failed");
                    vec![]
                }
            }
        }
        Ok(resp) => {
            tracing::debug!(url = %url, status = %resp.status(), "MCP target discovery returned non-success");
            vec![]
        }
        Err(e) => {
            tracing::debug!(error = %e, "MCP target discovery failed");
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
        .timeout(DISCOVERY_TIMEOUT)
        .build()?;
    let scrape_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_SCRAPES));
    let health = HealthState::new();
    let stats = Arc::new(AgentStats {
        scrape_errors: AtomicU64::new(0),
    });

    // Optional Valkey pool for provider liveness heartbeats.
    let valkey_pool: Option<fred::clients::Pool> = match &config.valkey_url {
        Some(url) => heartbeat::connect(url).await,
        None => {
            tracing::info!("VALKEY_URL not set — heartbeat push disabled");
            None
        }
    };

    // Optional PgPool for orphan job sweeper.
    let pg_pool: Option<sqlx::PgPool> = match &config.database_url {
        Some(url) => match sqlx::PgPool::connect(url).await {
            Ok(pool) => {
                tracing::info!("orphan sweeper: connected to PostgreSQL");
                Some(pool)
            }
            Err(e) => {
                tracing::warn!(error = %e, "DATABASE_URL set but connection failed — orphan sweeper disabled");
                None
            }
        },
        None => {
            tracing::info!("DATABASE_URL not set — orphan sweeper disabled");
            None
        }
    };

    // Spawn orphan sweeper when both Valkey and PgPool are available.
    let shutdown = CancellationToken::new();
    if let (Some(vk), Some(pg)) = (&valkey_pool, &pg_pool) {
        let vk = vk.clone();
        let pg = pg.clone();
        let ordinal = config.ordinal;
        let replicas = config.replicas_fallback;
        let token = shutdown.child_token();
        tokio::spawn(async move {
            orphan_sweeper::run_orphan_sweeper(vk, pg, ordinal, replicas, token).await;
        });
        tracing::info!("orphan sweeper spawned");
    }

    // Start health HTTP server
    let health_clone = health.clone();
    let health_port = config.health_port;
    tokio::spawn(async move {
        if let Err(e) = health::serve(health_port, health_clone).await {
            tracing::error!(error = %e, "health server failed");
        }
    });

    tracing::info!(
        api = %config.veronex_api_url,
        otel = %config.otel_endpoint,
        ordinal = config.ordinal,
        replicas_fallback = config.replicas_fallback,
        hostname = %config.hostname,
        interval_ms = config.scrape_interval.as_millis() as u64,
        health_port = health_port,
        "veronex-agent started"
    );

    loop {
        // ── Dynamic replica count: register self + read SCARD ────────
        let replicas = if let Some(ref pool) = valkey_pool {
            heartbeat::register_agent(pool, &config.hostname, HEARTBEAT_TTL_SECS).await;
            heartbeat::dynamic_replicas(pool, config.replicas_fallback).await
        } else {
            config.replicas_fallback
        };

        tokio::select! {
            biased;
            _ = signal::ctrl_c() => {
                tracing::info!("shutdown signal received");
                health.alive.store(false, Ordering::Relaxed);
                if let Some(ref pool) = valkey_pool {
                    heartbeat::deregister_agent(pool, &config.hostname).await;
                }
                shutdown.cancel();
                break;
            }
            result = async {
                tracing::info!(replicas, "scrape_cycle starting (with 120s timeout)");
                // Global timeout on the entire scrape cycle — prevents infinite hang
                match tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    scrape_cycle(&client, &config, replicas, &scrape_semaphore, valkey_pool.as_ref(), config.embed_url.as_deref())
                ).await {
                    Ok(r) => r,
                    Err(_) => {
                        tracing::error!("scrape_cycle timed out after 120s — skipping this cycle");
                        CycleResult { duration_secs: 120.0, targets_scraped: 0, gauges_collected: 0, success: false }
                    }
                }
            } => {
                // Update health state
                health.started.store(true, Ordering::Relaxed);
                health.ready.store(result.success, Ordering::Relaxed);
                if !result.success {
                    stats.scrape_errors.fetch_add(1, Ordering::Relaxed);
                }

                // Push self-metrics
                let self_gauges = agent_self_gauges(&stats, &result);
                if !self_gauges.is_empty() {
                    let self_labels: HashMap<String, String> =
                        [("service.name".into(), "veronex-agent".into())].into();
                    if let Err(e) = otlp::push_metrics(&client, &config.otel_endpoint, &self_labels, &self_gauges).await {
                        tracing::debug!(error = %e, "self-metrics push failed");
                    }
                }
            }
        }
        tokio::time::sleep(config.scrape_interval).await;
    }

    tracing::info!("veronex-agent stopped");
    Ok(())
}

// ── Self-metrics ────────────────────────────────────────────────────────────

fn agent_self_gauges(stats: &AgentStats, cycle: &CycleResult) -> Vec<scraper::Gauge> {
    vec![
        scraper::Gauge { name: "veronex_agent_up".into(), value: 1.0, labels: vec![] },
        scraper::Gauge { name: "veronex_agent_scrape_duration_seconds".into(), value: cycle.duration_secs, labels: vec![] },
        scraper::Gauge { name: "veronex_agent_scrape_targets_total".into(), value: cycle.targets_scraped as f64, labels: vec![] },
        scraper::Gauge { name: "veronex_agent_gauges_collected_total".into(), value: cycle.gauges_collected as f64, labels: vec![] },
        scraper::Gauge { name: "veronex_agent_scrape_errors_total".into(), value: stats.scrape_errors.load(Ordering::Relaxed) as f64, labels: vec![] },
    ]
}

/// Shard key for a target — server_id for servers, provider_id for ollama.
fn shard_key(t: &SdTarget) -> &str {
    match t.labels.get("type").map(|s| s.as_str()) {
        Some("server") => t.labels.get("server_id").map(|s| s.as_str()).unwrap_or(""),
        Some("ollama") => t.labels.get("provider_id").map(|s| s.as_str()).unwrap_or(""),
        _ => "",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn self_gauges_always_five() {
        let stats = AgentStats { scrape_errors: AtomicU64::new(0) };
        let cycle = CycleResult { duration_secs: 0.0, targets_scraped: 0, gauges_collected: 0, success: true };
        let gauges = agent_self_gauges(&stats, &cycle);
        assert_eq!(gauges.len(), 5);
        let names: Vec<&str> = gauges.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"veronex_agent_up"));
        assert!(names.contains(&"veronex_agent_scrape_duration_seconds"));
        assert!(names.contains(&"veronex_agent_scrape_targets_total"));
        assert!(names.contains(&"veronex_agent_gauges_collected_total"));
        assert!(names.contains(&"veronex_agent_scrape_errors_total"));
    }

    proptest! {
        /// Self-gauges reflect cycle values accurately for any input.
        #[test]
        fn self_gauges_reflect_values(
            duration in 0.0_f64..1000.0,
            targets in 0_usize..100,
            collected in 0_usize..10000,
            errors in 0_u64..1000,
        ) {
            let stats = AgentStats { scrape_errors: AtomicU64::new(errors) };
            let cycle = CycleResult { duration_secs: duration, targets_scraped: targets, gauges_collected: collected, success: true };
            let gauges = agent_self_gauges(&stats, &cycle);
            let find = |name: &str| gauges.iter().find(|g| g.name == name).unwrap().value;

            prop_assert_eq!(find("veronex_agent_up"), 1.0);
            prop_assert!((find("veronex_agent_scrape_duration_seconds") - duration).abs() < f64::EPSILON);
            prop_assert_eq!(find("veronex_agent_scrape_targets_total"), targets as f64);
            prop_assert_eq!(find("veronex_agent_gauges_collected_total"), collected as f64);
            prop_assert_eq!(find("veronex_agent_scrape_errors_total"), errors as f64);
        }
    }

    #[test]
    fn health_state_defaults() {
        let state = HealthState::new();
        assert!(!state.started.load(Ordering::Relaxed));
        assert!(!state.ready.load(Ordering::Relaxed));
        assert!(state.alive.load(Ordering::Relaxed));
    }

}

async fn scrape_cycle(
    client: &reqwest::Client,
    config: &Config,
    replicas: u32,
    semaphore: &Arc<Semaphore>,
    valkey: Option<&fred::clients::Pool>,
    embed_url: Option<&str>,
) -> CycleResult {
    let start = Instant::now();

    // ── MCP server health checks ─────────────────────────────────────────────
    // Fetches enabled MCP servers from /v1/mcp/targets on every cycle (dynamic).
    // Pings run concurrently — idempotent Valkey SET EX, no sharding needed.
    if let Some(pool) = valkey {
        let mcp_targets = fetch_mcp_targets(client, &config.veronex_api_url).await;
        if !mcp_targets.is_empty() {
            let ping_futs: Vec<_> = mcp_targets
                .iter()
                .cloned()
                .map(|(server_id, base_url)| async move {
                    let alive = scraper::ping_mcp_server(client, &server_id, &base_url).await;
                    (server_id, alive)
                })
                .collect();

            let ping_results = futures::future::join_all(ping_futs).await;
            let mut online_targets = Vec::new();
            for (server_id, alive) in ping_results {
                if alive {
                    scraper::set_mcp_heartbeat(pool, &server_id, HEARTBEAT_TTL_SECS).await;
                    if let Some(url) = mcp_targets.iter().find(|(id, _)| id == &server_id).map(|(_, u)| u.clone()) {
                        online_targets.push((server_id, url));
                    }
                } else {
                    tracing::debug!(server_id, "MCP server offline — heartbeat not renewed");
                }
            }

            // Tool discovery + embedding for online servers
            if let Some(eu) = embed_url {
                if !online_targets.is_empty() {
                    let targets_ref: Vec<(String, String)> = online_targets;
                    mcp_discover::discover_and_embed(client, pool, &targets_ref, eu).await;
                }
            }
        }
    }

    let targets = discover_targets(client, &config.veronex_api_url).await;
    if targets.is_empty() {
        tracing::debug!("no targets discovered");
        return CycleResult { duration_secs: start.elapsed().as_secs_f64(), targets_scraped: 0, gauges_collected: 0, success: true };
    }

    let my_targets: Vec<_> = targets
        .iter()
        .filter(|t| shard::owns(shard_key(t), config.ordinal, replicas))
        .collect();

    if my_targets.is_empty() {
        return CycleResult { duration_secs: start.elapsed().as_secs_f64(), targets_scraped: 0, gauges_collected: 0, success: true };
    }

    let targets_scraped = my_targets.len();

    let futures: Vec<_> = my_targets
        .iter()
        .filter_map(|t| {
            let host_port = t.targets.first()?;
            let target_type = t.labels.get("type")?.as_str();
            // targets API now returns scheme-preserving URLs
            let url = host_port.to_string();
            let labels = t.labels.clone();
            let sem = semaphore.clone();
            let valkey = valkey.cloned();

            Some(async move {
                let _permit = sem.acquire().await;
                match target_type {
                    "server" => {
                        let metrics = scraper::scrape_node_exporter(client, &url).await;
                        (labels, metrics)
                    }
                    "ollama" => {
                        let raw = scraper::scrape_ollama_raw(client, &url).await;
                        let metrics = scraper::ollama_gauges_from_raw(&raw);
                        // Push heartbeat + capacity state when scrape succeeded.
                        if let (Some(pool), Some(provider_id)) = (&valkey, labels.get("provider_id")) {
                            if !raw.is_empty() || metrics.iter().any(|g| g.name == "ollama_loaded_models") {
                                heartbeat::set_online(pool, provider_id, HEARTBEAT_TTL_SECS).await;
                            }
                            // Push capacity state (arch profiles) even when no models are loaded —
                            // this lets the analyzer skip HTTP /api/ps + /api/show calls.
                            let total_vram_mb: u64 = labels
                                .get("total_vram_mb")
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            let loaded: Vec<(String, u64)> = raw
                                .iter()
                                .filter_map(|m| Some((m.name.clone()?, m.size_vram.unwrap_or(0))))
                                .collect();
                            capacity_push::push(client, pool, &url, provider_id, total_vram_mb, &loaded).await;
                        }
                        (labels, metrics)
                    }
                    _ => (labels, vec![]),
                }
            })
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    let mut gauges_collected = 0;
    let mut any_error = false;

    for (labels, metrics) in &results {
        gauges_collected += metrics.len();
        if metrics.is_empty() {
            continue;
        }
        if let Err(e) = otlp::push_metrics(client, &config.otel_endpoint, labels, metrics).await {
            tracing::warn!(target_type = %labels.get("type").map(|s| s.as_str()).unwrap_or("unknown"), error = %e, "OTLP push failed");
            any_error = true;
        }
    }

    CycleResult {
        duration_secs: start.elapsed().as_secs_f64(),
        targets_scraped,
        gauges_collected,
        success: !any_error,
    }
}
