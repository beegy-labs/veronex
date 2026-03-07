use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

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
use veronex::domain::enums::AccountRole;
use veronex::domain::value_objects::JobStatusEvent;
use veronex::infrastructure::inbound::http::router::build_app;
use veronex::infrastructure::inbound::http::state::AppState;
use veronex::infrastructure::outbound::valkey_adapter::ValkeyAdapter;
use veronex::infrastructure::outbound::analytics::HttpAnalyticsClient;
use veronex::infrastructure::outbound::capacity::analyzer::run_sync_loop;
use veronex::infrastructure::outbound::session_grouping::run_session_grouping_loop;
use veronex::infrastructure::outbound::provider_dispatch::ConcreteProviderDispatch;
use veronex::infrastructure::outbound::capacity::vram_pool::VramPool;
use veronex::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use veronex::infrastructure::outbound::circuit_breaker::CircuitBreakerMap;
use veronex::infrastructure::outbound::health_checker::run_health_checker_loop;
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
use veronex::infrastructure::outbound::persistence::caching_model_selection::CachingModelSelection;
use veronex::infrastructure::outbound::persistence::caching_ollama_model_repo::CachingOllamaModelRepo;
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
        .expect("DATABASE_URL env var is required");

    let valkey_url = std::env::var("VALKEY_URL").ok();

    let analytics_url = std::env::var("ANALYTICS_URL").ok();
    let analytics_secret = std::env::var("ANALYTICS_SECRET").ok();

    let ollama_url = std::env::var("OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let _gemini_api_key = std::env::var("GEMINI_API_KEY").ok();

    let jwt_secret = std::env::var("JWT_SECRET")
        .expect("JWT_SECRET env var is required — generate with: openssl rand -hex 32");
    assert!(
        jwt_secret.len() >= 32,
        "JWT_SECRET must be at least 32 characters long (got {})",
        jwt_secret.len()
    );

    // Optional: set both to pre-seed a super account (CI/automated deployments).
    // When not set, the first-run setup flow (POST /v1/setup) is used instead.
    let bootstrap_super_user = std::env::var("BOOTSTRAP_SUPER_USER").ok().filter(|s| !s.is_empty());
    let bootstrap_super_pass = std::env::var("BOOTSTRAP_SUPER_PASS").ok().filter(|s| !s.is_empty());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let cors_raw = std::env::var("CORS_ALLOWED_ORIGINS")
        .expect("CORS_ALLOWED_ORIGINS env var is required (use comma-separated origins or 'none')");
    let cors_origins = parse_cors_origins(&cors_raw);

    // ── PostgreSQL ─────────────────────────────────────────────────
    let masked_db_url = mask_database_url(&database_url);
    tracing::info!("connecting to postgres at {masked_db_url}");
    let pg_pool = database::connect(&database_url).await?;
    sqlx::migrate!().run(&pg_pool).await?;
    tracing::info!("postgres ready, migrations applied");

    // ── Valkey (optional) ──────────────────────────────────────────
    let valkey_pool = if let Some(ref url) = valkey_url {
        use fred::prelude::*;
        tracing::info!("connecting to valkey at {url}");
        let config = Config::from_url(url)?;
        // Pool size configurable via VALKEY_POOL_SIZE (default: 6).
        // Valkey DB index is determined by the URL path (e.g. redis://host:6379/0).
        // If sharing Valkey with other services, always specify a DB index in the URL.
        let valkey_pool_size: usize = std::env::var("VALKEY_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);
        let pool = Pool::new(config, None, None, None, valkey_pool_size)?;
        pool.init().await?;
        tracing::info!("valkey ready");
        Some(pool)
    } else {
        tracing::warn!("VALKEY_URL not set — rate limiting and session revocation disabled");
        None
    };

    // ── Valkey port (application-layer abstraction) ───────────────
    let valkey_port: Option<Arc<dyn veronex::application::ports::outbound::valkey_port::ValkeyPort>> =
        valkey_pool.as_ref().map(|pool| {
            Arc::new(ValkeyAdapter::new(pool.clone())) as Arc<dyn veronex::application::ports::outbound::valkey_port::ValkeyPort>
        });

    // ── Shared HTTP client ──────────────────────────────────────────
    let http_client = reqwest::Client::new();

    // ── Observability adapter (HTTP → veronex-analytics) ───────────
    let observability: Option<Arc<dyn ObservabilityPort>> =
        match (&analytics_url, &analytics_secret) {
            (Some(url), Some(secret)) => {
                tracing::info!("http observability adapter enabled (analytics: {url})");
                Some(Arc::new(HttpObservabilityAdapter::new(http_client.clone(), url, secret)))
            }
            (Some(_), None) => {
                tracing::warn!("ANALYTICS_URL set but ANALYTICS_SECRET missing — observability disabled");
                None
            }
            _ => {
                tracing::warn!("ANALYTICS_URL not set — inference events will not be recorded");
                None
            }
        };

    // ── Audit adapter (HTTP → veronex-analytics) ───────────────────
    let audit_port: Option<Arc<dyn AuditPort>> =
        match (&analytics_url, &analytics_secret) {
            (Some(url), Some(secret)) => {
                tracing::info!("http audit adapter enabled");
                Some(Arc::new(HttpAuditAdapter::new(http_client.clone(), url, secret)))
            }
            (Some(_), None) => None, // already warned above
            _ => {
                tracing::warn!("ANALYTICS_URL not set — audit events will not be recorded");
                None
            }
        };

    // ── Analytics repository (HTTP → veronex-analytics) ───────────
    let analytics_repo: Option<Arc<dyn AnalyticsRepository>> =
        match (&analytics_url, &analytics_secret) {
            (Some(url), Some(secret)) => {
                tracing::info!("analytics repository enabled (analytics: {url})");
                Some(Arc::new(HttpAnalyticsClient::new(http_client.clone(), url, secret)))
            }
            (Some(_), None) => None, // already warned above
            _ => {
                tracing::warn!("ANALYTICS_URL not set — usage/performance/audit queries disabled");
                None
            }
        };

    // ── S3 / MinIO message store ───────────────────────────────────
    let s3_endpoint = std::env::var("S3_ENDPOINT").ok();
    let message_store: Option<Arc<dyn MessageStore>> = if let Some(ref endpoint) = s3_endpoint {
        let access_key = std::env::var("S3_ACCESS_KEY")
            .expect("S3_ACCESS_KEY is required");
        let secret_key = std::env::var("S3_SECRET_KEY")
            .expect("S3_SECRET_KEY is required");
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
    // Model manager disabled: VramPool + Ollama's OLLAMA_KEEP_ALIVE=-1 handle model
    // lifecycle. OllamaModelManager's ensure_loaded() sends keep_alive=0 which physically
    // unloads models, destroying multi-model co-residence.
    let model_manager: Option<Arc<dyn ModelManagerPort>> = None;
    tracing::info!("model manager disabled — VramPool manages model lifecycle");

    // ── Repositories ───────────────────────────────────────────────
    let account_repo: Arc<dyn AccountRepository> =
        Arc::new(PostgresAccountRepository::new(pg_pool.clone()));
    let api_key_repo = Arc::new(PostgresApiKeyRepository::new(pg_pool.clone()));
    let job_repo = Arc::new(PostgresJobRepository::new(pg_pool.clone()));
    let provider_registry: Arc<dyn LlmProviderRegistry> = Arc::new(
        CachingProviderRegistry::new(
            Arc::new(PostgresProviderRegistry::new(pg_pool.clone())),
            veronex::domain::constants::PROVIDER_REGISTRY_CACHE_TTL,
        )
    );
    let gpu_server_registry: Arc<dyn veronex::application::ports::outbound::gpu_server_registry::GpuServerRegistry> =
        Arc::new(PostgresGpuServerRegistry::new(pg_pool.clone()));
    let gemini_policy_repo: Arc<dyn veronex::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository> =
        Arc::new(PostgresGeminiPolicyRepository::new(pg_pool.clone()));
    let model_selection_repo: Arc<dyn veronex::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository> =
        Arc::new(CachingModelSelection::new(
            Arc::new(PostgresProviderModelSelectionRepository::new(pg_pool.clone())),
        ));
    let gemini_sync_config_repo: Arc<dyn veronex::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository> =
        Arc::new(PostgresGeminiSyncConfigRepository::new(pg_pool.clone()));
    let gemini_model_repo: Arc<dyn veronex::application::ports::outbound::gemini_model_repository::GeminiModelRepository> =
        Arc::new(PostgresGeminiModelRepository::new(pg_pool.clone()));
    let ollama_model_repo: Arc<dyn veronex::application::ports::outbound::ollama_model_repository::OllamaModelRepository> =
        Arc::new(CachingOllamaModelRepo::new(
            Arc::new(PostgresOllamaModelRepository::new(pg_pool.clone())),
        ));
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
        assert!(
            pass.len() >= 16,
            "BOOTSTRAP_SUPER_PASS must be at least 16 characters"
        );
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
                            role: AccountRole::Super,
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

    // ── Instance identity (multi-instance coordination) ──────────────
    let instance_id: Arc<str> = Arc::from(uuid::Uuid::new_v4().to_string());
    tracing::info!(instance_id = %instance_id, "instance identity generated");

    // ── Capacity infrastructure ─────────────────────────────────────
    let capacity_repo: Arc<dyn ModelCapacityRepository> =
        Arc::new(PostgresModelCapacityRepository::new(pg_pool.clone()));
    let capacity_settings_repo: Arc<dyn CapacitySettingsRepository> =
        Arc::new(PostgresCapacitySettingsRepository::new(pg_pool.clone()));
    use veronex::application::ports::outbound::concurrency_port::VramPoolPort;
    let vram_pool: Arc<dyn VramPoolPort> = if let Some(ref pool) = valkey_pool {
        Arc::new(veronex::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool::new(
            pool.clone(),
            instance_id.clone(),
        ))
    } else {
        Arc::new(VramPool::new())
    };
    // Restore learned max_concurrent / baseline_tps from DB into VramPool.
    if let Ok(profiles) = capacity_repo.list_all().await {
        for p in &profiles {
            if p.max_concurrent > 0 {
                vram_pool.set_max_concurrent(p.provider_id, &p.model_name, p.max_concurrent as u32);
            }
            if p.baseline_tps > 0 {
                vram_pool.set_baseline_tps(p.provider_id, &p.model_name, p.baseline_tps as u32);
            }
            if p.baseline_p95_ms > 0 {
                vram_pool.set_baseline_p95_ms(p.provider_id, &p.model_name, p.baseline_p95_ms as u32);
            }
        }
        if !profiles.is_empty() {
            tracing::info!(count = profiles.len(), "restored AIMD profiles from DB");
        }
    }

    let thermal         = Arc::new(ThermalThrottleMap::new(60)); // 60s cooldown
    let circuit_breaker = Arc::new(CircuitBreakerMap::new());
    let sync_trigger = Arc::new(tokio::sync::Notify::new());
    let sync_lock    = Arc::new(tokio::sync::Semaphore::new(1));

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
        http_client.clone(),
    ));

    // Providers are registered manually via API — no auto-seeding.

    // ── Sync loop (unified: health + models + VRAM) ──────────────────
    tasks.spawn(run_sync_loop(
        provider_registry.clone(),
        capacity_repo.clone(),
        capacity_settings_repo.clone(),
        vram_pool.clone(),
        valkey_pool.clone(),
        sync_trigger.clone(),
        sync_lock.clone(),
        veronex::domain::constants::SYNC_LOOP_BASE_TICK,
        shutdown.child_token(),
        http_client.clone(),
        ollama_model_repo.clone(),
        model_selection_repo.clone(),
    ));
    tracing::info!("sync loop started (analyzer: {analyzer_url})");

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
        valkey_port,
        observability,
        model_manager,
        vram_pool.clone(),
        thermal.clone(),
        circuit_breaker.clone(),
        provider_dispatch,
        (*job_event_tx).clone(),
        message_store.clone(),
        Some(ollama_model_repo.clone()),
        Some(model_selection_repo.clone()),
        instance_id.clone(),
    ));

    if let Err(e) = use_case_impl.recover_pending_jobs().await {
        tracing::warn!("job recovery failed (non-fatal): {e}");
    }
    tasks.spawn(use_case_impl.start_queue_worker(shutdown.child_token()));
    tasks.spawn(use_case_impl.start_job_sweeper(shutdown.child_token()));

    // ── Multi-instance pub/sub + reaper (when Valkey is available) ──
    if let Some(ref pool) = valkey_pool {
        use fred::clients::SubscriberClient;
        use fred::interfaces::ClientLike;
        use veronex::infrastructure::outbound::pubsub::{reaper, relay};

        // Job event subscriber: forwards events from other instances to local broadcast.
        let event_sub_config = fred::types::config::Config::from_url(
            &std::env::var("VALKEY_URL").unwrap_or_default(),
        ).unwrap();
        let event_subscriber = SubscriberClient::new(event_sub_config, None, None, None);
        event_subscriber.init().await.ok();
        let event_tx_clone = (*job_event_tx).clone();
        let iid = instance_id.clone();
        tasks.spawn(relay::run_job_event_subscriber(
            event_subscriber,
            event_tx_clone,
            iid,
            shutdown.child_token(),
        ));

        // Cancel subscriber: fires cancel_notify on local jobs via pub/sub.
        let cancel_sub_config = fred::types::config::Config::from_url(
            &std::env::var("VALKEY_URL").unwrap_or_default(),
        ).unwrap();
        let cancel_subscriber = SubscriberClient::new(cancel_sub_config, None, None, None);
        cancel_subscriber.init().await.ok();
        let cancel_notifiers = use_case_impl.cancel_notifiers();
        tasks.spawn(relay::run_cancel_subscriber(
            cancel_subscriber,
            cancel_notifiers,
            shutdown.child_token(),
        ));

        // Reaper: heartbeat + VRAM lease reaping + orphaned job re-enqueue.
        let distributed_vram_pool = {
            Some(Arc::new(
                veronex::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool::new(
                    pool.clone(),
                    instance_id.clone(),
                )
            ))
        };
        tasks.spawn(reaper::run_reaper_loop(
            pool.clone(),
            instance_id.clone(),
            distributed_vram_pool,
            shutdown.child_token(),
        ));

        tracing::info!("multi-instance coordination enabled (pub/sub + reaper)");
    }

    let use_case: Arc<dyn InferenceUseCase> = use_case_impl;

    let state = AppState {
        http_client,
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
        vram_pool,
        thermal,
        capacity_repo,
        capacity_settings_repo,
        sync_trigger,
        analyzer_url,
        job_event_tx,
        lab_settings_repo,
        circuit_breaker,
        message_store,
        session_grouping_lock,
        sync_lock,
        sse_connections: Arc::new(std::sync::atomic::AtomicU32::new(0)),
    };

    let app = build_app(state, cors_origins);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("veronex listening on {addr}");
    let shutdown_clone = shutdown.clone();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = shutdown_signal() => {}
                _ = shutdown_clone.cancelled() => {}
            }
        })
        .await?;

    tracing::info!("shutting down background tasks...");
    shutdown.cancel();
    let drain = async {
        while let Some(res) = tasks.join_next().await {
            if let Err(e) = res {
                tracing::warn!("background task panicked: {e:?}");
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(30), drain).await.is_err() {
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

/// Mask the password in a database URL for safe logging.
///
/// `postgres://user:secret@host:5432/db` → `postgres://user:***@host:5432/db`
fn mask_database_url(url: &str) -> String {
    // Find `://user:password@` and replace the password portion with `***`.
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
    // No password found — return as-is (no credentials to leak).
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
