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
use veronex::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use veronex::application::ports::outbound::model_capacity_repository::ModelCapacityRepository;
use veronex::application::ports::outbound::model_manager_port::ModelManagerPort;
use veronex::application::ports::outbound::observability_port::ObservabilityPort;
use veronex::application::ports::outbound::session_repository::SessionRepository;
use veronex::application::use_cases::InferenceUseCaseImpl;
use veronex::domain::enums::ProviderType;
use veronex::domain::value_objects::JobStatusEvent;
use veronex::infrastructure::inbound::http::router::build_app;
use veronex::infrastructure::inbound::http::state::AppState;
use veronex::infrastructure::outbound::analytics::HttpAnalyticsClient;
use veronex::infrastructure::outbound::capacity::analyzer::run_capacity_analysis_loop;
use veronex::infrastructure::outbound::session_grouping::run_session_grouping_loop;
use veronex::infrastructure::outbound::provider_dispatch::ConcreteProviderDispatch;
use veronex::infrastructure::outbound::capacity::slot_map::ConcurrencySlotMap;
use veronex::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use veronex::infrastructure::outbound::circuit_breaker::CircuitBreakerMap;
use veronex::infrastructure::outbound::health_checker::{check_backend, run_health_checker_loop};
use veronex::infrastructure::outbound::model_manager::OllamaModelManager;
use veronex::infrastructure::outbound::observability::{HttpAuditAdapter, HttpObservabilityAdapter};
use veronex::infrastructure::outbound::persistence::account_repository::PostgresAccountRepository;
use veronex::infrastructure::outbound::persistence::api_key_repository::PostgresApiKeyRepository;
use veronex::infrastructure::outbound::persistence::provider_model_selection::PostgresProviderModelSelectionRepository;
use veronex::infrastructure::outbound::persistence::provider_registry::PostgresProviderRegistry;
use veronex::infrastructure::outbound::persistence::caching_provider_registry::CachingProviderRegistry;
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
use veronex::application::ports::outbound::message_store::MessageStore;
use veronex::infrastructure::outbound::s3::message_store::S3MessageStore;

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
        .expect("JWT_SECRET env var is required — generate with: openssl rand -hex 32");
    assert!(
        jwt_secret.len() >= 32,
        "JWT_SECRET must be at least 32 characters long (got {})",
        jwt_secret.len()
    );

    // Optional: set both to pre-seed a super account (CI/automated deployments).
    // When not set, the first-run setup flow (POST /v1/setup) is used instead.
    let bootstrap_super_user = std::env::var("BOOTSTRAP_SUPER_USER").ok();
    let bootstrap_super_pass = std::env::var("BOOTSTRAP_SUPER_PASS").ok();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let cors_raw = std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_else(|_| "*".to_string());
    let cors_origins = parse_cors_origins(&cors_raw);

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

    // ── S3 / MinIO message store ───────────────────────────────────
    let s3_endpoint = std::env::var("S3_ENDPOINT").ok();
    let message_store: Option<Arc<dyn MessageStore>> = if let Some(ref endpoint) = s3_endpoint {
        let access_key = std::env::var("S3_ACCESS_KEY")
            .unwrap_or_else(|_| "veronex".to_string());
        let secret_key = std::env::var("S3_SECRET_KEY")
            .unwrap_or_else(|_| "veronex123".to_string());
        let bucket = std::env::var("S3_BUCKET")
            .unwrap_or_else(|_| "veronex-messages".to_string());
        let region = std::env::var("S3_REGION")
            .unwrap_or_else(|_| "us-east-1".to_string());

        use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
        let creds = Credentials::new(&access_key, &secret_key, None, None, "veronex");
        let s3_config = aws_sdk_s3::Config::builder()
            .endpoint_url(endpoint)
            .region(Region::new(region))
            .credentials_provider(creds)
            .force_path_style(true) // required for MinIO path-style access
            .behavior_version(BehaviorVersion::latest())
            .build();
        let s3_client = aws_sdk_s3::Client::from_conf(s3_config);
        let store = S3MessageStore::new(s3_client, &bucket);

        // Ensure bucket exists (idempotent — safe to call on every startup)
        if let Err(e) = store.ensure_bucket().await {
            tracing::warn!("S3 bucket init failed (non-fatal): {e}");
        }

        tracing::info!("S3 message store enabled (endpoint={endpoint}, bucket={bucket})");
        Some(Arc::new(store))
    } else {
        tracing::warn!("S3_ENDPOINT not set — conversation contexts stored in PostgreSQL only");
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
    let provider_registry: Arc<dyn LlmProviderRegistry> = Arc::new(
        CachingProviderRegistry::new(
            Arc::new(PostgresProviderRegistry::new(pg_pool.clone())),
            Duration::from_secs(5),
        )
    );
    let gpu_server_registry: Arc<dyn veronex::application::ports::outbound::gpu_server_registry::GpuServerRegistry> =
        Arc::new(PostgresGpuServerRegistry::new(pg_pool.clone()));
    let gemini_policy_repo: Arc<dyn veronex::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository> =
        Arc::new(PostgresGeminiPolicyRepository::new(pg_pool.clone()));
    let model_selection_repo: Arc<dyn veronex::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository> =
        Arc::new(PostgresProviderModelSelectionRepository::new(pg_pool.clone()));
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
    let slot_map        = Arc::new(ConcurrencySlotMap::new());
    let thermal         = Arc::new(ThermalThrottleMap::new(60)); // 60s cooldown
    let circuit_breaker = Arc::new(CircuitBreakerMap::new());
    let capacity_manual_trigger = Arc::new(tokio::sync::Notify::new());
    let capacity_analysis_lock  = Arc::new(tokio::sync::Semaphore::new(1));

    // ── Shutdown token + task set ───────────────────────────────────
    let shutdown = CancellationToken::new();
    let mut tasks: JoinSet<()> = JoinSet::new();

    // ── Background provider health checker ──────────────────────────
    tasks.spawn(run_health_checker_loop(
        provider_registry.clone(),
        30,
        valkey_pool.clone(),
        thermal.clone(),
        shutdown.child_token(),
    ));

    // ── Auto-seed providers ─────────────────────────────────────────
    let http_client = reqwest::Client::new();
    {
        let existing = provider_registry.list_all().await.unwrap_or_default();
        let has_ollama = existing.iter().any(|b| matches!(b.provider_type, ProviderType::Ollama));
        if !has_ollama {
            let ollama_provider = veronex::domain::entities::LlmProvider {
                id: uuid::Uuid::now_v7(),
                name: "local-ollama".to_string(),
                provider_type: ProviderType::Ollama,
                url: ollama_url.clone(),
                api_key_encrypted: None,
                is_active: true,
                total_vram_mb: 0,
                gpu_index: None,
                server_id: None,
                agent_url: None,
                is_free_tier: false,
                status: veronex::domain::enums::LlmProviderStatus::Offline,
                registered_at: chrono::Utc::now(),
            };
            let initial_status = check_backend(&http_client, &ollama_provider).await;
            let ollama_provider = veronex::domain::entities::LlmProvider {
                status: initial_status,
                ..ollama_provider
            };
            if let Err(e) = provider_registry.register(&ollama_provider).await {
                tracing::warn!("failed to auto-register ollama provider: {e}");
            } else {
                tracing::info!("auto-registered ollama provider at {ollama_url}");
            }
        }

        if let Some(ref key) = gemini_api_key {
            let has_gemini = existing.iter().any(|b| matches!(b.provider_type, ProviderType::Gemini));
            if !has_gemini {
                let gemini_provider = veronex::domain::entities::LlmProvider {
                    id: uuid::Uuid::now_v7(),
                    name: "gemini".to_string(),
                    provider_type: ProviderType::Gemini,
                    url: String::new(),
                    api_key_encrypted: Some(key.clone()),
                    is_active: true,
                    total_vram_mb: 0,
                    gpu_index: None,
                    server_id: None,
                    agent_url: None,
                    is_free_tier: false,
                    status: veronex::domain::enums::LlmProviderStatus::Offline,
                    registered_at: chrono::Utc::now(),
                };
                let initial_status = check_backend(&http_client, &gemini_provider).await;
                let gemini_provider = veronex::domain::entities::LlmProvider {
                    status: initial_status,
                    ..gemini_provider
                };
                if let Err(e) = provider_registry.register(&gemini_provider).await {
                    tracing::warn!("failed to auto-register gemini provider: {e}");
                } else {
                    tracing::info!("auto-registered gemini provider");
                }
            }
        }
    }

    // ── Capacity analysis loop (5-min background) ──────────────────
    tasks.spawn(run_capacity_analysis_loop(
        provider_registry.clone(),
        capacity_repo.clone(),
        capacity_settings_repo.clone(),
        slot_map.clone(),
        valkey_pool.clone(),
        analyzer_url.clone(),
        capacity_manual_trigger.clone(),
        capacity_analysis_lock.clone(),
        Duration::from_secs(30), // base tick (checks DB settings each tick)
        shutdown.child_token(),
        ollama_num_parallel,
    ));
    tracing::info!("capacity analysis loop started (analyzer: {analyzer_url})");

    // ── Session grouping loop (daily background) ────────────────────
    // Groups inference_jobs into conversations via messages_prefix_hash chaining.
    // No LLM, no race conditions — runs on completed job history once per day.
    let session_grouping_interval_secs: u64 = std::env::var("SESSION_GROUPING_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(86_400); // 24h default
    let session_grouping_lock = Arc::new(tokio::sync::Semaphore::new(1));
    tasks.spawn(run_session_grouping_loop(
        Arc::new(pg_pool.clone()),
        session_grouping_lock.clone(),
        Duration::from_secs(session_grouping_interval_secs),
        shutdown.child_token(),
    ));
    tracing::info!("session grouping loop started (interval={session_grouping_interval_secs}s)");

    let (job_event_tx, _) = tokio::sync::broadcast::channel::<JobStatusEvent>(256);
    let job_event_tx = Arc::new(job_event_tx);

    let provider_dispatch = Arc::new(ConcreteProviderDispatch::new(
        provider_registry.clone(),
        Some(gemini_policy_repo.clone()),
        Some(model_selection_repo.clone()),
        Some(ollama_model_repo.clone()),
        valkey_pool.clone(),
    ));

    let use_case_impl = Arc::new(InferenceUseCaseImpl::new(
        provider_registry.clone(),
        job_repo,
        valkey_pool.clone(),
        observability,
        model_manager,
        slot_map.clone(),
        thermal.clone(),
        circuit_breaker.clone(),
        provider_dispatch,
        (*job_event_tx).clone(),
        message_store.clone(),
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
        provider_registry,
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
        circuit_breaker,
        message_store,
        session_grouping_lock,
        capacity_analysis_lock,
    };

    let app = build_app(state, cors_origins);

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

/// Parse `CORS_ALLOWED_ORIGINS` into a list of `HeaderValue`s.
///
/// `"*"` (default) → empty Vec → `AllowOrigin::any()` in the router.
/// `"https://a.com,https://b.com"` → list of two origins.
fn parse_cors_origins(raw: &str) -> Vec<axum::http::HeaderValue> {
    if raw.trim() == "*" {
        return Vec::new();
    }
    raw.split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() { return None; }
            axum::http::HeaderValue::from_str(s).ok()
        })
        .collect()
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
