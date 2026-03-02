use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use veronex::application::ports::inbound::inference_use_case::InferenceUseCase;
use veronex::application::ports::outbound::account_repository::AccountRepository;
use veronex::application::ports::outbound::analytics_repository::AnalyticsRepository;
use veronex::application::ports::outbound::audit_port::AuditPort;
use veronex::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository;
use veronex::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use veronex::application::ports::outbound::model_capacity_repository::ModelCapacityRepository;
use veronex::application::ports::outbound::model_manager_port::ModelManagerPort;
use veronex::application::ports::outbound::observability_port::ObservabilityPort;
use veronex::application::ports::outbound::session_repository::SessionRepository;
use veronex::application::use_cases::InferenceUseCaseImpl;
use veronex::domain::enums::BackendType;
use veronex::domain::value_objects::JobStatusEvent;
use veronex::infrastructure::inbound::http::router::build_app;
use veronex::infrastructure::inbound::http::state::AppState;
use veronex::infrastructure::outbound::analytics::HttpAnalyticsClient;
use veronex::infrastructure::outbound::capacity::analyzer::run_capacity_analysis_loop;
use veronex::infrastructure::outbound::capacity::slot_map::ConcurrencySlotMap;
use veronex::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use veronex::infrastructure::outbound::health_checker::{check_backend, run_health_checker_loop};
use veronex::infrastructure::outbound::model_manager::OllamaModelManager;
use veronex::infrastructure::outbound::observability::{HttpAuditAdapter, HttpObservabilityAdapter};
use veronex::infrastructure::outbound::persistence::account_repository::PostgresAccountRepository;
use veronex::infrastructure::outbound::persistence::api_key_repository::PostgresApiKeyRepository;
use veronex::infrastructure::outbound::persistence::backend_model_selection::PostgresBackendModelSelectionRepository;
use veronex::infrastructure::outbound::persistence::backend_registry::PostgresBackendRegistry;
use veronex::infrastructure::outbound::persistence::caching_backend_registry::CachingBackendRegistry;
use veronex::infrastructure::outbound::persistence::capacity_settings_repository::PostgresCapacitySettingsRepository;
use veronex::infrastructure::outbound::persistence::database;
use veronex::infrastructure::outbound::persistence::gemini_model_repository::PostgresGeminiModelRepository;
use veronex::infrastructure::outbound::persistence::gemini_policy_repository::PostgresGeminiPolicyRepository;
use veronex::infrastructure::outbound::persistence::gemini_sync_config::PostgresGeminiSyncConfigRepository;
use veronex::infrastructure::outbound::persistence::gpu_server_registry::PostgresGpuServerRegistry;
use veronex::infrastructure::outbound::persistence::job_repository::PostgresJobRepository;
use veronex::infrastructure::outbound::persistence::model_capacity_repository::PostgresModelCapacityRepository;
use veronex::infrastructure::outbound::persistence::ollama_model_repository::PostgresOllamaModelRepository;
use veronex::infrastructure::outbound::persistence::ollama_sync_job_repository::PostgresOllamaSyncJobRepository;
use veronex::infrastructure::outbound::persistence::session_repository::PostgresSessionRepository;

// ── Entry point ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // ── Config ─────────────────────────────────────────────────────
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://veronex:veronex@localhost:5433/veronex".to_string());

    let valkey_url = std::env::var("VALKEY_URL").ok();

    let analytics_url = std::env::var("ANALYTICS_URL").ok();
    let analytics_secret = std::env::var("ANALYTICS_SECRET")
        .unwrap_or_else(|_| "veronex-analytics-internal-secret".to_string());

    let ollama_url = std::env::var("OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let gemini_api_key = std::env::var("GEMINI_API_KEY").ok();

    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "change-me-in-production".to_string());

    // Optional: set both to pre-seed a super account (CI/automated deployments).
    // When not set, the first-run setup flow (POST /v1/setup) is used instead.
    let bootstrap_super_user = std::env::var("BOOTSTRAP_SUPER_USER").ok();
    let bootstrap_super_pass = std::env::var("BOOTSTRAP_SUPER_PASS").ok();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    // ── PostgreSQL ─────────────────────────────────────────────────
    tracing::info!("connecting to postgres at {database_url}");
    let pg_pool = database::connect(&database_url).await?;
    sqlx::migrate!().run(&pg_pool).await?;
    tracing::info!("postgres ready, migrations applied");

    // ── Valkey (optional) ──────────────────────────────────────────
    let valkey_pool = if let Some(ref url) = valkey_url {
        use fred::prelude::*;
        tracing::info!("connecting to valkey at {url}");
        let config = Config::from_url(url)?;
        let pool = Pool::new(config, None, None, None, 6)?;
        pool.init().await?;
        tracing::info!("valkey ready");
        Some(pool)
    } else {
        tracing::warn!("VALKEY_URL not set — rate limiting and session revocation disabled");
        None
    };

    // ── Observability adapter (HTTP → veronex-analytics) ───────────
    let observability: Option<Arc<dyn ObservabilityPort>> =
        if let Some(ref url) = analytics_url {
            tracing::info!("http observability adapter enabled (analytics: {url})");
            Some(Arc::new(HttpObservabilityAdapter::new(url, &analytics_secret)))
        } else {
            tracing::warn!("ANALYTICS_URL not set — inference events will not be recorded");
            None
        };

    // ── Audit adapter (HTTP → veronex-analytics) ───────────────────
    let audit_port: Option<Arc<dyn AuditPort>> =
        if let Some(ref url) = analytics_url {
            tracing::info!("http audit adapter enabled");
            Some(Arc::new(HttpAuditAdapter::new(url, &analytics_secret)))
        } else {
            tracing::warn!("ANALYTICS_URL not set — audit events will not be recorded");
            None
        };

    // ── Analytics repository (HTTP → veronex-analytics) ───────────
    let analytics_repo: Option<Arc<dyn AnalyticsRepository>> =
        if let Some(ref url) = analytics_url {
            tracing::info!("analytics repository enabled (analytics: {url})");
            Some(Arc::new(HttpAnalyticsClient::new(url, &analytics_secret)))
        } else {
            tracing::warn!("ANALYTICS_URL not set — usage/performance/audit queries disabled");
            None
        };

    // ── Model manager ──────────────────────────────────────────────
    let model_manager: Option<Arc<dyn ModelManagerPort>> =
        Some(Arc::new(OllamaModelManager::new(&ollama_url, 1)));
    tracing::info!("model manager enabled (max_loaded=1)");

    // ── Repositories ───────────────────────────────────────────────
    let account_repo: Arc<dyn AccountRepository> =
        Arc::new(PostgresAccountRepository::new(pg_pool.clone()));
    let api_key_repo = Arc::new(PostgresApiKeyRepository::new(pg_pool.clone()));
    let job_repo = Arc::new(PostgresJobRepository::new(pg_pool.clone()));
    let backend_registry: Arc<dyn LlmBackendRegistry> = Arc::new(
        CachingBackendRegistry::new(
            Arc::new(PostgresBackendRegistry::new(pg_pool.clone())),
            Duration::from_secs(5),
        )
    );
    let gpu_server_registry: Arc<dyn veronex::application::ports::outbound::gpu_server_registry::GpuServerRegistry> =
        Arc::new(PostgresGpuServerRegistry::new(pg_pool.clone()));
    let gemini_policy_repo: Arc<dyn veronex::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository> =
        Arc::new(PostgresGeminiPolicyRepository::new(pg_pool.clone()));
    let model_selection_repo: Arc<dyn veronex::application::ports::outbound::backend_model_selection::BackendModelSelectionRepository> =
        Arc::new(PostgresBackendModelSelectionRepository::new(pg_pool.clone()));
    let gemini_sync_config_repo: Arc<dyn veronex::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository> =
        Arc::new(PostgresGeminiSyncConfigRepository::new(pg_pool.clone()));
    let gemini_model_repo: Arc<dyn veronex::application::ports::outbound::gemini_model_repository::GeminiModelRepository> =
        Arc::new(PostgresGeminiModelRepository::new(pg_pool.clone()));
    let ollama_model_repo: Arc<dyn veronex::application::ports::outbound::ollama_model_repository::OllamaModelRepository> =
        Arc::new(PostgresOllamaModelRepository::new(pg_pool.clone()));
    let ollama_sync_job_repo: Arc<dyn veronex::application::ports::outbound::ollama_sync_job_repository::OllamaSyncJobRepository> =
        Arc::new(PostgresOllamaSyncJobRepository::new(pg_pool.clone()));
    let session_repo: Arc<dyn SessionRepository> =
        Arc::new(PostgresSessionRepository::new(pg_pool.clone()));
    let lab_settings_repo: Arc<dyn veronex::application::ports::outbound::lab_settings_repository::LabSettingsRepository> =
        Arc::new(veronex::infrastructure::outbound::persistence::lab_settings_repository::PostgresLabSettingsRepository::new(pg_pool.clone()));

    // ── Bootstrap super account (optional — CI/automated deployments) ──────
    // When BOOTSTRAP_SUPER_USER + BOOTSTRAP_SUPER_PASS are set, a super account
    // is pre-seeded on startup. Otherwise, use POST /v1/setup for first-run setup.
    if let (Some(user), Some(pass)) = (bootstrap_super_user, bootstrap_super_pass) {
        match account_repo.get_by_username(&user).await {
            Ok(Some(_)) => tracing::debug!("bootstrap super account already exists"),
            Ok(None) => {
                use argon2::{
                    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
                    Argon2,
                };
                let salt = SaltString::generate(&mut OsRng);
                match Argon2::default()
                    .hash_password(pass.as_bytes(), &salt)
                    .map(|h| h.to_string())
                {
                    Ok(hash) => {
                        let super_account = veronex::domain::entities::Account {
                            id: uuid::Uuid::now_v7(),
                            username: user.clone(),
                            password_hash: hash,
                            name: "Super Admin".to_string(),
                            email: None,
                            role: "super".to_string(),
                            department: None,
                            position: None,
                            is_active: true,
                            created_by: None,
                            last_login_at: None,
                            created_at: chrono::Utc::now(),
                            deleted_at: None,
                        };
                        match account_repo.create(&super_account).await {
                            Ok(()) => tracing::info!("bootstrap super account '{user}' created"),
                            Err(e) => tracing::warn!("failed to create bootstrap super account: {e}"),
                        }
                    }
                    Err(e) => tracing::warn!("failed to hash bootstrap super password: {e}"),
                }
            }
            Err(e) => tracing::warn!("failed to check bootstrap super account: {e}"),
        }
    }

    // ── Capacity analyzer URL ───────────────────────────────────────
    let analyzer_url = std::env::var("CAPACITY_ANALYZER_OLLAMA_URL")
        .unwrap_or_else(|_| ollama_url.clone());

    // OLLAMA_NUM_PARALLEL caps the slot ceiling in the capacity analyzer.
    // Must match the Ollama StatefulSet env var (default: 1 for AMD APU).
    let ollama_num_parallel: u32 = std::env::var("OLLAMA_NUM_PARALLEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    // ── Capacity infrastructure ─────────────────────────────────────
    let capacity_repo: Arc<dyn ModelCapacityRepository> =
        Arc::new(PostgresModelCapacityRepository::new(pg_pool.clone()));
    let capacity_settings_repo: Arc<dyn CapacitySettingsRepository> =
        Arc::new(PostgresCapacitySettingsRepository::new(pg_pool.clone()));
    let slot_map   = Arc::new(ConcurrencySlotMap::new());
    let thermal    = Arc::new(ThermalThrottleMap::new(60)); // 60s cooldown
    let capacity_manual_trigger = Arc::new(tokio::sync::Notify::new());

    // ── Shutdown token + task set ───────────────────────────────────
    let shutdown = CancellationToken::new();
    let mut tasks: JoinSet<()> = JoinSet::new();

    // ── Background backend health checker ──────────────────────────
    tasks.spawn(run_health_checker_loop(
        backend_registry.clone(),
        30,
        valkey_pool.clone(),
        thermal.clone(),
        shutdown.child_token(),
    ));

    // ── Auto-seed backends ─────────────────────────────────────────
    let http_client = reqwest::Client::new();
    {
        let existing = backend_registry.list_all().await.unwrap_or_default();
        let has_ollama = existing.iter().any(|b| matches!(b.backend_type, BackendType::Ollama));
        if !has_ollama {
            let ollama_backend = veronex::domain::entities::LlmBackend {
                id: uuid::Uuid::now_v7(),
                name: "local-ollama".to_string(),
                backend_type: BackendType::Ollama,
                url: ollama_url.clone(),
                api_key_encrypted: None,
                is_active: true,
                total_vram_mb: 0,
                gpu_index: None,
                server_id: None,
                agent_url: None,
                is_free_tier: false,
                status: veronex::domain::enums::LlmBackendStatus::Offline,
                registered_at: chrono::Utc::now(),
            };
            let initial_status = check_backend(&http_client, &ollama_backend).await;
            let ollama_backend = veronex::domain::entities::LlmBackend {
                status: initial_status,
                ..ollama_backend
            };
            if let Err(e) = backend_registry.register(&ollama_backend).await {
                tracing::warn!("failed to auto-register ollama backend: {e}");
            } else {
                tracing::info!("auto-registered ollama backend at {ollama_url}");
            }
        }

        if let Some(ref key) = gemini_api_key {
            let has_gemini = existing.iter().any(|b| matches!(b.backend_type, BackendType::Gemini));
            if !has_gemini {
                let gemini_backend = veronex::domain::entities::LlmBackend {
                    id: uuid::Uuid::now_v7(),
                    name: "gemini".to_string(),
                    backend_type: BackendType::Gemini,
                    url: String::new(),
                    api_key_encrypted: Some(key.clone()),
                    is_active: true,
                    total_vram_mb: 0,
                    gpu_index: None,
                    server_id: None,
                    agent_url: None,
                    is_free_tier: false,
                    status: veronex::domain::enums::LlmBackendStatus::Offline,
                    registered_at: chrono::Utc::now(),
                };
                let initial_status = check_backend(&http_client, &gemini_backend).await;
                let gemini_backend = veronex::domain::entities::LlmBackend {
                    status: initial_status,
                    ..gemini_backend
                };
                if let Err(e) = backend_registry.register(&gemini_backend).await {
                    tracing::warn!("failed to auto-register gemini backend: {e}");
                } else {
                    tracing::info!("auto-registered gemini backend");
                }
            }
        }
    }

    // ── Capacity analysis loop (5-min background) ──────────────────
    tasks.spawn(run_capacity_analysis_loop(
        backend_registry.clone(),
        capacity_repo.clone(),
        capacity_settings_repo.clone(),
        slot_map.clone(),
        valkey_pool.clone(),
        analyzer_url.clone(),
        capacity_manual_trigger.clone(),
        Duration::from_secs(30), // base tick (checks DB settings each tick)
        shutdown.child_token(),
        ollama_num_parallel,
    ));
    tracing::info!("capacity analysis loop started (analyzer: {analyzer_url})");

    let (job_event_tx, _) = tokio::sync::broadcast::channel::<JobStatusEvent>(256);
    let job_event_tx = Arc::new(job_event_tx);

    let use_case_impl = Arc::new(InferenceUseCaseImpl::new(
        backend_registry.clone(),
        Some(gemini_policy_repo.clone()),
        Some(model_selection_repo.clone()),
        Some(ollama_model_repo.clone()),
        job_repo,
        valkey_pool.clone(),
        observability,
        model_manager,
        slot_map.clone(),
        thermal.clone(),
        (*job_event_tx).clone(),
    ));

    if let Err(e) = use_case_impl.recover_pending_jobs().await {
        tracing::warn!("job recovery failed (non-fatal): {e}");
    }
    tasks.spawn(use_case_impl.start_queue_worker(shutdown.child_token()));

    let use_case: Arc<dyn InferenceUseCase> = use_case_impl;

    let state = AppState {
        use_case,
        api_key_repo,
        account_repo,
        audit_port,
        jwt_secret,
        backend_registry,
        gpu_server_registry,
        gemini_policy_repo,
        gemini_sync_config_repo,
        gemini_model_repo,
        model_selection_repo,
        ollama_model_repo,
        ollama_sync_job_repo,
        valkey_pool,
        analytics_repo,
        session_repo,
        pg_pool,
        cpu_snapshot_cache: Arc::new(dashmap::DashMap::new()),
        slot_map,
        thermal,
        capacity_repo,
        capacity_settings_repo,
        capacity_manual_trigger,
        analyzer_url,
        job_event_tx,
        lab_settings_repo,
    };

    let app = build_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("veronex listening on {addr}");
    let shutdown_clone = shutdown.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = shutdown_signal() => {}
                _ = shutdown_clone.cancelled() => {}
            }
        })
        .await?;

    tracing::info!("shutting down background tasks...");
    shutdown.cancel();
    while let Some(res) = tasks.join_next().await {
        if let Err(e) = res {
            tracing::warn!("background task panicked: {e:?}");
        }
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
