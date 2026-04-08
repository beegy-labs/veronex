use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod bootstrap;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use veronex::domain::constants::MCP_TOOL_REFRESH_INTERVAL;

/// Maximum time to wait for background tasks during graceful shutdown.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
use tracing_subscriber::EnvFilter;

use veronex::infrastructure::inbound::http::router::build_app;
use veronex::infrastructure::inbound::http::state::AppState;
use veronex::infrastructure::outbound::persistence::database;

// ── Entry point ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = bootstrap::AppConfig::from_env();

    // ── PostgreSQL ─────────────────────────────────────────────────
    let masked_db_url = mask_database_url(&config.database_url);
    tracing::info!("connecting to postgres at {masked_db_url}");
    let pg_pool = database::connect(&config.database_url).await?;
    tracing::info!("postgres ready");

    // ── Valkey (optional) ──────────────────────────────────────────
    let valkey_pool = if let Some(ref url) = config.valkey_url {
        use fred::prelude::*;
        tracing::info!("connecting to valkey at {url}");
        let valkey_config = Config::from_url(url)?;
        let valkey_pool_size: usize = std::env::var("VALKEY_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);
        let pool = Pool::new(valkey_config, None, None, None, valkey_pool_size)?;
        pool.init().await?;
        tracing::info!("valkey ready");
        Some(pool)
    } else {
        tracing::warn!("VALKEY_URL not set — rate limiting and session revocation disabled");
        None
    };

    // ── Infrastructure context ─────────────────────────────────────
    let instance_id: Arc<str> = Arc::from(
        std::env::var("VERONEX_INSTANCE_ID")
            .unwrap_or_else(|_| uuid::Uuid::now_v7().to_string()),
    );
    tracing::info!(instance_id = %instance_id, "instance identity generated");
    let infra = bootstrap::InfraContext {
        valkey_pool,
        pg_pool,
        http_client: reqwest::Client::new(),
        instance_id,
    };

    // ── Wire repositories ──────────────────────────────────────────
    let repos = bootstrap::wire_repositories(&infra, &config).await?;

    // ── Bootstrap super account ────────────────────────────────────
    bootstrap::repositories::maybe_bootstrap_super_account(&repos.account_repo, &config, &infra.pg_pool).await;

    // ── Background tasks ───────────────────────────────────────────
    let shutdown = CancellationToken::new();
    let mut tasks: JoinSet<()> = JoinSet::new();
    let handles = bootstrap::spawn_background_tasks(
        &repos,
        &config,
        &infra,
        &shutdown,
        &mut tasks,
    )
    .await;

    // ── Build app state ────────────────────────────────────────────
    let bootstrap::InfraContext { valkey_pool, pg_pool, http_client, instance_id } = infra;

    // ── Wire MCP vector selector (requires VESPA_URL + EMBED_URL) ─────
    let (mcp_vector_selector, mcp_tool_indexer) = {
        use veronex_mcp::vector::{EmbedClient, McpToolIndexer, McpVectorSelector, VespaClient};
        match (std::env::var("VESPA_URL").ok(), std::env::var("EMBED_URL").ok()) {
            (Some(vespa_url), Some(embed_url)) => {
                let vespa = VespaClient::new(&vespa_url);
                let embed = EmbedClient::new(&embed_url);
                let top_k = std::env::var("MCP_VECTOR_TOP_K")
                    .ok().and_then(|v| v.parse().ok()).unwrap_or(16usize);
                let valkey_arc = valkey_pool.as_ref()
                    .map(|v| std::sync::Arc::new(v.clone()));
                if let Some(valkey_arc) = valkey_arc {
                    let selector = McpVectorSelector::new(vespa.clone(), embed.clone(), valkey_arc, top_k);
                    let indexer = McpToolIndexer::new(vespa, embed);
                    tracing::info!(vespa_url, top_k, "MCP vector selector enabled");
                    (Some(std::sync::Arc::new(selector)), Some(std::sync::Arc::new(indexer)))
                } else {
                    tracing::warn!("MCP vector selector requires Valkey — disabled");
                    (None, None)
                }
            }
            _ => {
                tracing::info!("VESPA_URL/EMBED_URL not set — MCP vector selection disabled (fallback: get_all)");
                (None, None)
            }
        }
    };

    // ── Wire MCP bridge (requires Valkey) ──────────────────────────
    let mcp_bridge = if let Some(ref valkey) = valkey_pool {
        use std::sync::Arc;
        use veronex_mcp::{McpCircuitBreaker, McpHttpClient, McpResultCache, McpSessionManager, McpToolCache};
        use veronex::infrastructure::outbound::mcp::McpBridgeAdapter;
        let valkey_arc = Arc::new(valkey.clone());
        let session_mgr = Arc::new(McpSessionManager::new(McpHttpClient::new()));
        let tool_cache = Arc::new(McpToolCache::new(valkey_arc.clone(), veronex::infrastructure::outbound::mcp::bridge::MAX_TOOLS_PER_REQUEST));
        let result_cache = Arc::new(McpResultCache::new(valkey_arc));
        let circuit_breaker = Arc::new(McpCircuitBreaker::new());
        let bridge = McpBridgeAdapter {
            session_manager: session_mgr.clone(),
            tool_cache,
            result_cache,
            circuit_breaker,
            analytics_repo: repos.analytics_repo.clone(),
        };
        #[derive(sqlx::FromRow)]
        struct McpServerStartup { id: uuid::Uuid, slug: String, url: String, timeout_secs: i16 }
        let servers: Vec<McpServerStartup> = sqlx::query_as(
            "SELECT id, slug, url, timeout_secs FROM mcp_servers WHERE is_enabled = true"
        )
        .fetch_all(&pg_pool)
        .await
        .unwrap_or_default();
        for s in servers {
            if let Err(e) = session_mgr.connect(s.id, &s.slug, &s.url, s.timeout_secs as u16).await {
                tracing::warn!(id = %s.id, error = %e, "MCP startup connect failed");
            }
        }
        Some(Arc::new(bridge))
    } else {
        None
    };

    let state = AppState {
        http_client,
        use_case: handles.use_case,
        api_key_repo: repos.api_key_repo,
        account_repo: repos.account_repo,
        audit_port: repos.audit_port,
        jwt_secret: config.jwt_secret,
        provider_registry: repos.provider_registry,
        gpu_server_registry: repos.gpu_server_registry,
        gemini_policy_repo: repos.gemini_policy_repo,
        gemini_sync_config_repo: repos.gemini_sync_config_repo,
        gemini_model_repo: repos.gemini_model_repo,
        model_selection_repo: repos.model_selection_repo,
        global_model_settings_repo: repos.global_model_settings_repo,
        api_key_provider_access_repo: repos.api_key_provider_access_repo,
        ollama_model_repo: repos.ollama_model_repo,
        ollama_sync_job_repo: repos.ollama_sync_job_repo,
        valkey_pool,
        analytics_repo: repos.analytics_repo,
        session_repo: repos.session_repo,
        pg_pool,
        cpu_snapshot_cache: Arc::new(dashmap::DashMap::new()),
        vram_pool: repos.vram_pool,
        thermal: handles.thermal,
        capacity_repo: repos.capacity_repo,
        capacity_settings_repo: repos.capacity_settings_repo,
        sync_trigger: handles.sync_trigger,
        analyzer_url: config.analyzer_url,
        job_event_tx: handles.job_event_tx,
        event_ring_buffer: handles.event_ring_buffer,
        stats_tx: handles.stats_tx,
        lab_settings_repo: repos.lab_settings_repo,
        mcp_settings_repo: repos.mcp_settings_repo,
        circuit_breaker: handles.circuit_breaker,
        message_store: repos.message_store,
        image_store: repos.image_store,
        session_grouping_lock: handles.session_grouping_lock,
        sync_lock: handles.sync_lock,
        sse_connections: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        key_in_flight: Arc::new(dashmap::DashMap::new()),
        vram_budget_repo: repos.vram_budget_repo,
        mcp_bridge,
        mcp_vector_selector,
        mcp_tool_indexer,
        login_rate_limit: std::env::var("LOGIN_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
        instance_id,
        kafka_broker_admin_url: config.kafka_broker.as_ref().map(|broker| {
            // Convert kafka broker address to Redpanda admin URL.
            // e.g. "redpanda:9092" → "http://redpanda:9644"
            let host = broker.split(':').next().unwrap_or("redpanda");
            Arc::from(format!("http://{host}:9644").as_str())
        }),
        clickhouse_http_url: config.clickhouse_http_url.as_deref().map(Arc::from),
        clickhouse_user: config.clickhouse_user.as_deref().map(Arc::from),
        clickhouse_password: config.clickhouse_password.as_deref().map(Arc::from),
        clickhouse_db: config.clickhouse_db.as_deref().map(Arc::from),
    };

    // ── MCP tool refresh loop ──────────────────────────────────────
    // Periodically refresh tool cache for all connected MCP servers.
    // Interval (25s) keeps L2 Valkey entry alive before its 35s TTL.
    if state.mcp_bridge.is_some() {
        let state_clone = state.clone();
        let cancel_clone = shutdown.clone();
        tokio::spawn(async move {
            use veronex::infrastructure::inbound::http::mcp_handlers::discover_tools_startup;
            // Initial discovery on startup
            for server_id in state_clone.mcp_bridge.as_ref().map(|b| b.session_manager.server_ids()).unwrap_or_default() {
                discover_tools_startup(&state_clone, server_id).await;
            }
            // Periodic refresh
            let mut interval = tokio::time::interval(MCP_TOOL_REFRESH_INTERVAL);
            interval.tick().await; // skip the immediate tick
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Some(ref b) = state_clone.mcp_bridge {
                            for server_id in b.session_manager.server_ids() {
                                if let Some(tools) = b.tool_cache.refresh(server_id, &b.session_manager).await {
                                    if let Some(ref indexer) = state_clone.mcp_tool_indexer {
                                        let indexer = indexer.clone();
                                        tokio::spawn(async move {
                                            indexer.index_server_tools("global", server_id, &tools).await;
                                        });
                                    }
                                }
                            }
                        }
                    }
                    _ = cancel_clone.cancelled() => break,
                }
            }
        });
    }

    // Capture for shutdown deregister (state is moved into build_app).
    let shutdown_valkey = state.valkey_pool.clone();
    let shutdown_instance_id = state.instance_id.clone();

    let app = build_app(state, config.cors_origins);

    // ── Start server ───────────────────────────────────────────────
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("veronex listening on {addr}");
    let shutdown_clone = shutdown.clone();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = shutdown_signal() => {}
            _ = shutdown_clone.cancelled() => {}
        }
    })
    .await?;

    // ── Graceful shutdown ──────────────────────────────────────────
    tracing::info!("shutting down background tasks...");

    // Deregister this instance from Valkey before draining tasks.
    // Prevents ghost entries when pods restart (HPA, rolling deploy).
    if let Some(ref vk) = shutdown_valkey {
        use fred::prelude::*;
        use veronex::infrastructure::outbound::valkey_keys;
        let iid = shutdown_instance_id.as_ref();
        if let Err(e) = vk.srem::<i64, _, _>(valkey_keys::INSTANCES_SET, iid).await {
            tracing::warn!(error = %e, "Valkey SREM instances_set on shutdown failed");
        }
        if let Err(e) = vk.del::<i64, _>(valkey_keys::heartbeat(iid)).await {
            tracing::warn!(error = %e, "Valkey DEL heartbeat on shutdown failed");
        }
        if let Err(e) = vk.del::<i64, _>(valkey_keys::service_health(iid)).await {
            tracing::warn!(error = %e, "Valkey DEL service_health on shutdown failed");
        }
        tracing::info!("instance deregistered from Valkey");
    }

    shutdown.cancel();
    let drain = async {
        while let Some(res) = tasks.join_next().await {
            if let Err(e) = res {
                tracing::warn!("background task panicked: {e:?}");
            }
        }
    };
    if tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, drain)
        .await
        .is_err()
    {
        tracing::warn!("background tasks did not finish within 30s — forcing exit");
    }
    tracing::info!("shutdown complete");

    Ok(())
}

// ── OS signal handler ──────────────────────────────────────────────

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

// ── Tracing initialisation ─────────────────────────────────────────

fn init_tracing() {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer();

    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    if let Some(ref endpoint) = otel_endpoint {
        match build_otlp_tracer(endpoint) {
            Ok(tracer) => {
                let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(fmt_layer)
                    .with(otel_layer)
                    .init();
                eprintln!("[veronex] OTel OTLP tracing enabled, exporting to {endpoint}");
                return;
            }
            Err(e) => {
                eprintln!(
                    "[veronex] WARN: failed to initialise OTLP tracing (falling back to stdout): {e}"
                );
            }
        }
    }

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

/// Mask the password in a database URL for safe logging.
///
/// `postgres://user:secret@host:5432/db` → `postgres://user:***@host:5432/db`
fn mask_database_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            let userinfo = &after_scheme[..at_pos];
            if let Some(colon) = userinfo.find(':') {
                let user = &userinfo[..colon];
                let host_onward = &after_scheme[at_pos..];
                return format!("{}://{}:***{}", &url[..scheme_end], user, host_onward);
            }
        }
    }
    url.to_string()
}

fn build_otlp_tracer(endpoint: &str) -> anyhow::Result<opentelemetry_sdk::trace::SdkTracer> {
    use opentelemetry_otlp::{SpanExporter, WithExportConfig as _};
    use opentelemetry_sdk::trace::SdkTracerProvider;

    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    // runtime::Tokio argument removed in 0.31 — BatchSpanProcessor now uses its own background thread.
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    use opentelemetry::trace::TracerProvider as _;
    let tracer = provider.tracer("veronex");

    opentelemetry::global::set_tracer_provider(provider);

    Ok(tracer)
}
