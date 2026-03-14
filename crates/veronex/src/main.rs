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
    let instance_id: Arc<str> = Arc::from(uuid::Uuid::new_v4().to_string());
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
    bootstrap::repositories::maybe_bootstrap_super_account(&repos.account_repo, &config).await;

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
    let bootstrap::InfraContext { valkey_pool, pg_pool, http_client, .. } = infra;
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
        lab_settings_repo: repos.lab_settings_repo,
        circuit_breaker: handles.circuit_breaker,
        message_store: repos.message_store,
        session_grouping_lock: handles.session_grouping_lock,
        sync_lock: handles.sync_lock,
        sse_connections: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        vram_budget_repo: repos.vram_budget_repo,
    };

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
    shutdown.cancel();
    let drain = async {
        while let Some(res) = tasks.join_next().await {
            if let Err(e) = res {
                tracing::warn!("background task panicked: {e:?}");
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(30), drain)
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

fn build_otlp_tracer(endpoint: &str) -> anyhow::Result<opentelemetry_sdk::trace::Tracer> {
    use opentelemetry_otlp::{SpanExporter, WithExportConfig as _};
    use opentelemetry_sdk::runtime;
    use opentelemetry_sdk::trace::TracerProvider;

    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, runtime::Tokio)
        .build();

    use opentelemetry::trace::TracerProvider as _;
    let tracer = provider.tracer("veronex");

    opentelemetry::global::set_tracer_provider(provider);

    Ok(tracer)
}
