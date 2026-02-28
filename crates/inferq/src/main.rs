use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use veronex::application::ports::inbound::inference_use_case::InferenceUseCase;
use veronex::application::ports::outbound::api_key_repository::ApiKeyRepository;
use veronex::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use veronex::application::ports::outbound::model_manager_port::ModelManagerPort;
use veronex::application::ports::outbound::observability_port::ObservabilityPort;
use veronex::application::use_cases::InferenceUseCaseImpl;
use veronex::domain::entities::ApiKey;
use veronex::domain::enums::BackendType;
use veronex::domain::services::api_key_generator::hash_api_key;
use veronex::infrastructure::inbound::http::router::build_app;
use veronex::infrastructure::inbound::http::state::AppState;
use veronex::infrastructure::outbound::health_checker::{check_backend, start_health_checker};
use veronex::infrastructure::outbound::model_manager::OllamaModelManager;
use veronex::infrastructure::outbound::observability::RedpandaObservabilityAdapter;
use veronex::infrastructure::outbound::persistence::api_key_repository::PostgresApiKeyRepository;
use veronex::infrastructure::outbound::persistence::backend_model_selection::PostgresBackendModelSelectionRepository;
use veronex::infrastructure::outbound::persistence::backend_registry::PostgresBackendRegistry;
use veronex::infrastructure::outbound::persistence::database;
use veronex::infrastructure::outbound::persistence::gemini_model_repository::PostgresGeminiModelRepository;
use veronex::infrastructure::outbound::persistence::gemini_policy_repository::PostgresGeminiPolicyRepository;
use veronex::infrastructure::outbound::persistence::gemini_sync_config::PostgresGeminiSyncConfigRepository;
use veronex::infrastructure::outbound::persistence::gpu_server_registry::PostgresGpuServerRegistry;
use veronex::infrastructure::outbound::persistence::job_repository::PostgresJobRepository;
use veronex::infrastructure::outbound::persistence::ollama_model_repository::PostgresOllamaModelRepository;
use veronex::infrastructure::outbound::persistence::ollama_sync_job_repository::PostgresOllamaSyncJobRepository;

// ── Entry point ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // ── Tracing / OTel ──────────────────────────────────────────────
    //
    // If OTEL_EXPORTER_OTLP_ENDPOINT is set, attempt to initialise an
    // OTLP exporter and layer it on top of the normal stdout subscriber.
    // Failure to initialise OTel is non-fatal; we fall back to stdout.
    // Otherwise, just use plain stdout tracing.
    init_tracing();

    // ── Config ─────────────────────────────────────────────────────
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://veronex:veronex@localhost:5433/veronex".to_string());

    let valkey_url = std::env::var("VALKEY_URL").ok();

    let clickhouse_url =
        std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
    let clickhouse_user = std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "veronex".to_string());
    let clickhouse_password =
        std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_else(|_| "veronex".to_string());
    let clickhouse_db = std::env::var("CLICKHOUSE_DB").unwrap_or_else(|_| "veronex".to_string());
    let clickhouse_enabled = std::env::var("CLICKHOUSE_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let ollama_url = std::env::var("OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let gemini_api_key = std::env::var("GEMINI_API_KEY").ok();

    let redpanda_url = std::env::var("REDPANDA_URL")
        .unwrap_or_else(|_| "localhost:9092".to_string());

    let bootstrap_api_key = std::env::var("BOOTSTRAP_API_KEY").ok();

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
        let config = RedisConfig::from_url(url)?;
        let pool = RedisPool::new(config, None, None, None, 6)?;
        pool.connect();
        pool.wait_for_connect().await?;
        tracing::info!("valkey ready");
        Some(pool)
    } else {
        tracing::warn!("VALKEY_URL not set — rate limiting disabled");
        None
    };

    // ── ClickHouse (optional) ──────────────────────────────────────
    let clickhouse_client = if clickhouse_enabled {
        tracing::info!("connecting to clickhouse at {clickhouse_url}");
        let client = clickhouse::Client::default()
            .with_url(&clickhouse_url)
            .with_user(&clickhouse_user)
            .with_password(&clickhouse_password)
            .with_database(&clickhouse_db);
        tracing::info!("clickhouse ready");
        Some(client)
    } else {
        tracing::warn!("CLICKHOUSE_ENABLED not set — usage analytics disabled");
        None
    };

    // ── Observability adapter (Redpanda) ───────────────────────────
    // Inference events are produced to the 'inference' topic.
    // ClickHouse consumes via Kafka Engine → inference_logs MV.
    // Fail-open: if Redpanda is unavailable, observability is disabled.
    let observability: Option<Arc<dyn ObservabilityPort>> =
        match RedpandaObservabilityAdapter::new(vec![redpanda_url.clone()]).await {
            Ok(adapter) => {
                tracing::info!("redpanda observability adapter enabled (broker: {redpanda_url})");
                Some(Arc::new(adapter))
            }
            Err(e) => {
                tracing::warn!("redpanda observability adapter failed — inference events will not be recorded (non-fatal): {e}");
                None
            }
        };

    // ── Model manager (Ollama LRU eviction) ────────────────────────
    // max_loaded=1: single GPU, greedy allocation (OLLAMA_KEEP_ALIVE=-1).
    let model_manager: Option<Arc<dyn ModelManagerPort>> =
        Some(Arc::new(OllamaModelManager::new(&ollama_url, 1)));
    tracing::info!("model manager enabled (max_loaded=1, greedy allocation)");

    // ── Repositories ───────────────────────────────────────────────
    let api_key_repo = Arc::new(PostgresApiKeyRepository::new(pg_pool.clone()));
    let job_repo = Arc::new(PostgresJobRepository::new(pg_pool.clone()));
    let backend_registry: Arc<dyn LlmBackendRegistry> =
        Arc::new(PostgresBackendRegistry::new(pg_pool.clone()));
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

    // ── Bootstrap admin key ────────────────────────────────────────
    // If BOOTSTRAP_API_KEY is set, create it in the DB if it doesn't exist yet.
    if let Some(ref raw_key) = bootstrap_api_key {
        let key_hash = hash_api_key(raw_key);
        match api_key_repo.get_by_hash(&key_hash).await {
            Ok(Some(_)) => tracing::debug!("bootstrap admin key already exists"),
            Ok(None) => {
                let prefix = raw_key.chars().take(12).collect::<String>();
                let key = ApiKey {
                    id: uuid::Uuid::now_v7(),
                    key_hash,
                    key_prefix: prefix,
                    tenant_id: "admin".to_string(),
                    name: "bootstrap-admin".to_string(),
                    is_active: true,
                    rate_limit_rpm: 0,
                    rate_limit_tpm: 0,
                    expires_at: None,
                    created_at: chrono::Utc::now(),
                    deleted_at: None,
                };
                match api_key_repo.create(&key).await {
                    Ok(()) => tracing::info!("bootstrap admin key created"),
                    Err(e) => tracing::warn!("failed to create bootstrap admin key: {e}"),
                }
            }
            Err(e) => tracing::warn!("failed to check bootstrap admin key: {e}"),
        }
    }

    // ── Background backend health checker ──────────────────────────
    // Also polls veronex-agent metrics (GPU temp/VRAM/RAM) when agent_url is set,
    // and caches the result in Valkey for the VRAM-aware dispatcher.
    start_health_checker(backend_registry.clone(), 30, valkey_pool.clone());

    // ── Auto-seed backends from environment (optional convenience) ──────
    // If OLLAMA_URL or GEMINI_API_KEY are set, register them in the DB
    // if no backend of that type exists yet.
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

    let use_case_impl = Arc::new(InferenceUseCaseImpl::new(
        backend_registry.clone(), // registry used for VRAM-aware dynamic routing
        Some(gemini_policy_repo.clone()),
        Some(model_selection_repo.clone()),
        Some(ollama_model_repo.clone()),
        job_repo,
        valkey_pool.clone(),
        observability,
        model_manager,
    ));

    // ── Queue worker setup ─────────────────────────────────────────
    // Recover pending/running jobs from the previous run, then start
    // the serial queue worker (max_jobs=1, enforces single-GPU use).
    if let Err(e) = use_case_impl.recover_pending_jobs().await {
        tracing::warn!("job recovery failed (non-fatal): {e}");
    }
    use_case_impl.start_queue_worker();

    let use_case: Arc<dyn InferenceUseCase> = use_case_impl;

    let state = AppState {
        use_case,
        api_key_repo,
        backend_registry,
        gpu_server_registry,
        gemini_policy_repo,
        gemini_sync_config_repo,
        gemini_model_repo,
        model_selection_repo,
        ollama_model_repo,
        ollama_sync_job_repo,
        valkey_pool,
        clickhouse_client,
        pg_pool,
        cpu_snapshot_cache: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    let app = build_app(state);

    // ── Server ─────────────────────────────────────────────────────
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("veronex listening on {addr}");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ── Tracing initialisation ─────────────────────────────────────────

/// Set up the global tracing subscriber.
///
/// When `OTEL_EXPORTER_OTLP_ENDPOINT` is set a best-effort OTLP tracing
/// layer is added.  If that fails we log a warning and continue with
/// stdout-only tracing.  The function never panics.
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
                // Can't use tracing here — subscriber just initialised.
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

    // Fallback: stdout only.
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

/// Build an OTLP `opentelemetry` tracer that exports to `endpoint`.
///
/// Returns `opentelemetry_sdk::trace::Tracer` which implements `PreSampledTracer`
/// as required by `tracing_opentelemetry::layer().with_tracer(...)`.
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

    // Register as the global tracer provider.
    opentelemetry::global::set_tracer_provider(provider);

    Ok(tracer)
}
