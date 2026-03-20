use std::sync::Arc;

use veronex::application::ports::outbound::account_repository::AccountRepository;
use veronex::application::ports::outbound::analytics_repository::AnalyticsRepository;
use veronex::application::ports::outbound::api_key_repository::ApiKeyRepository;
use veronex::application::ports::outbound::audit_port::AuditPort;
use veronex::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository;
use veronex::application::ports::outbound::concurrency_port::VramPoolPort;
use veronex::application::ports::outbound::gemini_model_repository::GeminiModelRepository;
use veronex::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use veronex::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;
use veronex::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
use veronex::application::ports::outbound::lab_settings_repository::LabSettingsRepository;
use veronex::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use veronex::application::ports::outbound::image_store::ImageStore;
use veronex::application::ports::outbound::message_store::MessageStore;
use veronex::application::ports::outbound::model_capacity_repository::ModelCapacityRepository;
use veronex::application::ports::outbound::model_manager_port::ModelManagerPort;
use veronex::application::ports::outbound::observability_port::ObservabilityPort;
use veronex::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use veronex::application::ports::outbound::ollama_sync_job_repository::OllamaSyncJobRepository;
use veronex::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use veronex::application::ports::outbound::provider_vram_budget_repository::ProviderVramBudgetRepository;
use veronex::application::ports::outbound::session_repository::SessionRepository;
use veronex::infrastructure::outbound::analytics::HttpAnalyticsClient;
use veronex::infrastructure::outbound::capacity::vram_pool::VramPool;
use veronex::infrastructure::outbound::observability::{HttpAuditAdapter, HttpObservabilityAdapter};
use veronex::infrastructure::outbound::persistence::account_repository::PostgresAccountRepository;
use veronex::infrastructure::outbound::persistence::api_key_repository::PostgresApiKeyRepository;
use veronex::infrastructure::outbound::persistence::caching_model_selection::CachingModelSelection;
use veronex::infrastructure::outbound::persistence::caching_ollama_model_repo::CachingOllamaModelRepo;
use veronex::infrastructure::outbound::persistence::caching_provider_registry::CachingProviderRegistry;
use veronex::infrastructure::outbound::persistence::capacity_settings_repository::PostgresCapacitySettingsRepository;
use veronex::infrastructure::outbound::persistence::gemini_model_repository::PostgresGeminiModelRepository;
use veronex::infrastructure::outbound::persistence::gemini_policy_repository::PostgresGeminiPolicyRepository;
use veronex::infrastructure::outbound::persistence::gemini_sync_config::PostgresGeminiSyncConfigRepository;
use veronex::infrastructure::outbound::persistence::gpu_server_registry::PostgresGpuServerRegistry;
use veronex::infrastructure::outbound::persistence::job_repository::PostgresJobRepository;
use veronex::infrastructure::outbound::persistence::lab_settings_repository::PostgresLabSettingsRepository;
use veronex::infrastructure::outbound::persistence::model_capacity_repository::PostgresModelCapacityRepository;
use veronex::infrastructure::outbound::persistence::ollama_model_repository::PostgresOllamaModelRepository;
use veronex::infrastructure::outbound::persistence::ollama_sync_job_repository::PostgresOllamaSyncJobRepository;
use veronex::infrastructure::outbound::persistence::provider_model_selection::PostgresProviderModelSelectionRepository;
use veronex::infrastructure::outbound::persistence::provider_registry::PostgresProviderRegistry;
use veronex::infrastructure::outbound::persistence::provider_vram_budget_repository::PostgresProviderVramBudgetRepository;
use veronex::infrastructure::outbound::persistence::session_repository::PostgresSessionRepository;
use veronex::infrastructure::outbound::s3::image_store::S3ImageStore;
use veronex::infrastructure::outbound::s3::message_store::S3MessageStore;
use veronex::infrastructure::outbound::valkey_adapter::ValkeyAdapter;

use super::background::InfraContext;
use super::config::AppConfig;

/// All wired repository instances needed by the application.
pub struct Repositories {
    pub account_repo: Arc<dyn AccountRepository>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub job_repo: Arc<PostgresJobRepository>,
    pub provider_registry: Arc<dyn LlmProviderRegistry>,
    pub gpu_server_registry: Arc<dyn GpuServerRegistry>,
    pub gemini_policy_repo: Arc<dyn GeminiPolicyRepository>,
    pub model_selection_repo: Arc<dyn ProviderModelSelectionRepository>,
    pub gemini_sync_config_repo: Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo: Arc<dyn GeminiModelRepository>,
    pub ollama_model_repo: Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo: Arc<dyn OllamaSyncJobRepository>,
    pub session_repo: Arc<dyn SessionRepository>,
    pub lab_settings_repo: Arc<dyn LabSettingsRepository>,
    pub capacity_repo: Arc<dyn ModelCapacityRepository>,
    pub capacity_settings_repo: Arc<dyn CapacitySettingsRepository>,
    pub valkey_port: Option<Arc<dyn veronex::application::ports::outbound::valkey_port::ValkeyPort>>,
    pub observability: Option<Arc<dyn ObservabilityPort>>,
    pub audit_port: Option<Arc<dyn AuditPort>>,
    pub analytics_repo: Option<Arc<dyn AnalyticsRepository>>,
    pub model_manager: Option<Arc<dyn ModelManagerPort>>,
    pub vram_pool: Arc<dyn VramPoolPort>,
    pub message_store: Option<Arc<dyn MessageStore>>,
    pub image_store: Option<Arc<dyn ImageStore>>,
    pub vram_budget_repo: Arc<dyn ProviderVramBudgetRepository>,
}

/// Wire all repositories from database pools and configuration.
pub async fn wire_repositories(
    infra: &InfraContext,
    config: &AppConfig,
) -> anyhow::Result<Repositories> {
    let pg_pool = &infra.pg_pool;
    let valkey_pool = &infra.valkey_pool;
    let http_client = &infra.http_client;
    let instance_id = &infra.instance_id;

    // ── Valkey port ────────────────────────────────────────────────
    let valkey_port: Option<Arc<dyn veronex::application::ports::outbound::valkey_port::ValkeyPort>> =
        valkey_pool.as_ref().map(|pool| {
            Arc::new(ValkeyAdapter::new(pool.clone()))
                as Arc<dyn veronex::application::ports::outbound::valkey_port::ValkeyPort>
        });

    // ── Observability adapter ──────────────────────────────────────
    let observability: Option<Arc<dyn ObservabilityPort>> =
        match (&config.analytics_url, &config.analytics_secret) {
            (Some(url), Some(secret)) => {
                tracing::info!("http observability adapter enabled (analytics: {url})");
                Some(Arc::new(HttpObservabilityAdapter::new(
                    http_client.clone(),
                    url,
                    secret,
                )))
            }
            (Some(_), None) => {
                tracing::warn!(
                    "ANALYTICS_URL set but ANALYTICS_SECRET missing — observability disabled"
                );
                None
            }
            _ => {
                tracing::warn!("ANALYTICS_URL not set — inference events will not be recorded");
                None
            }
        };

    // ── Audit adapter ──────────────────────────────────────────────
    let audit_port: Option<Arc<dyn AuditPort>> =
        match (&config.analytics_url, &config.analytics_secret) {
            (Some(url), Some(secret)) => {
                tracing::info!("http audit adapter enabled");
                Some(Arc::new(HttpAuditAdapter::new(
                    http_client.clone(),
                    url,
                    secret,
                )))
            }
            (Some(_), None) => None, // already warned above
            _ => {
                tracing::warn!("ANALYTICS_URL not set — audit events will not be recorded");
                None
            }
        };

    // ── Analytics repository ───────────────────────────────────────
    let analytics_repo: Option<Arc<dyn AnalyticsRepository>> =
        match (&config.analytics_url, &config.analytics_secret) {
            (Some(url), Some(secret)) => {
                tracing::info!("analytics repository enabled (analytics: {url})");
                Some(Arc::new(HttpAnalyticsClient::new(
                    http_client.clone(),
                    url,
                    secret,
                )))
            }
            (Some(_), None) => None, // already warned above
            _ => {
                tracing::warn!(
                    "ANALYTICS_URL not set — usage/performance/audit queries disabled"
                );
                None
            }
        };

    // ── S3 / MinIO message store ───────────────────────────────────
    let s3_endpoint = std::env::var("S3_ENDPOINT").ok();
    let message_store: Option<Arc<dyn MessageStore>> = if let Some(ref endpoint) = s3_endpoint {
        let access_key = std::env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY is required");
        let secret_key = std::env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY is required");
        let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "veronex-messages".to_string());
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());

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

    // ── S3 / MinIO image store ────────────────────────────────────
    let image_store: Option<Arc<dyn ImageStore>> = if let Some(ref endpoint) = s3_endpoint {
        let access_key = std::env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY is required");
        let secret_key = std::env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY is required");
        let bucket = std::env::var("S3_IMAGE_BUCKET").unwrap_or_else(|_| "veronex-images".to_string());
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());

        use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
        let creds = Credentials::new(&access_key, &secret_key, None, None, "veronex");
        let s3_config = aws_sdk_s3::Config::builder()
            .endpoint_url(endpoint)
            .region(Region::new(region))
            .credentials_provider(creds)
            .force_path_style(true)
            .behavior_version(BehaviorVersion::latest())
            .build();
        let s3_client = aws_sdk_s3::Client::from_conf(s3_config);
        let endpoint_url = std::env::var("S3_IMAGE_PUBLIC_URL")
            .unwrap_or_else(|_| format!("{}/{}", endpoint.trim_end_matches('/'), &bucket));
        let store = S3ImageStore::new(s3_client, &bucket, endpoint_url);

        if let Err(e) = store.ensure_bucket().await {
            tracing::warn!("S3 image bucket init failed (non-fatal): {e}");
        }

        tracing::info!("S3 image store enabled (endpoint={endpoint}, bucket={bucket})");
        Some(Arc::new(store))
    } else {
        None
    };

    // ── Model manager ──────────────────────────────────────────────
    let model_manager: Option<Arc<dyn ModelManagerPort>> = None;
    tracing::info!("model manager disabled — VramPool manages model lifecycle");

    // ── Postgres repositories ──────────────────────────────────────
    let account_repo: Arc<dyn AccountRepository> =
        Arc::new(PostgresAccountRepository::new(pg_pool.clone()));
    let api_key_repo: Arc<dyn ApiKeyRepository> =
        Arc::new(PostgresApiKeyRepository::new(pg_pool.clone()));
    let job_repo = Arc::new(PostgresJobRepository::new(pg_pool.clone()));
    let provider_registry: Arc<dyn LlmProviderRegistry> = Arc::new(CachingProviderRegistry::new(
        Arc::new(PostgresProviderRegistry::new(pg_pool.clone(), config.gemini_encryption_key)),
        veronex::domain::constants::PROVIDER_REGISTRY_CACHE_TTL,
    ));
    let gpu_server_registry: Arc<dyn GpuServerRegistry> =
        Arc::new(PostgresGpuServerRegistry::new(pg_pool.clone()));
    let gemini_policy_repo: Arc<dyn GeminiPolicyRepository> =
        Arc::new(PostgresGeminiPolicyRepository::new(pg_pool.clone()));
    let model_selection_repo: Arc<dyn ProviderModelSelectionRepository> =
        Arc::new(CachingModelSelection::new(Arc::new(
            PostgresProviderModelSelectionRepository::new(pg_pool.clone()),
        )));
    let gemini_sync_config_repo: Arc<dyn GeminiSyncConfigRepository> =
        Arc::new(PostgresGeminiSyncConfigRepository::new(pg_pool.clone(), config.gemini_encryption_key));
    let gemini_model_repo: Arc<dyn GeminiModelRepository> =
        Arc::new(PostgresGeminiModelRepository::new(pg_pool.clone()));
    let ollama_model_repo: Arc<dyn OllamaModelRepository> =
        Arc::new(CachingOllamaModelRepo::new(Arc::new(
            PostgresOllamaModelRepository::new(pg_pool.clone()),
        )));
    let ollama_sync_job_repo: Arc<dyn OllamaSyncJobRepository> =
        Arc::new(PostgresOllamaSyncJobRepository::new(pg_pool.clone()));
    let session_repo: Arc<dyn SessionRepository> =
        Arc::new(PostgresSessionRepository::new(pg_pool.clone()));
    let lab_settings_repo: Arc<dyn LabSettingsRepository> =
        Arc::new(PostgresLabSettingsRepository::new(pg_pool.clone()));

    // ── Capacity infrastructure ────────────────────────────────────
    let capacity_repo: Arc<dyn ModelCapacityRepository> =
        Arc::new(PostgresModelCapacityRepository::new(pg_pool.clone()));
    let capacity_settings_repo: Arc<dyn CapacitySettingsRepository> =
        Arc::new(PostgresCapacitySettingsRepository::new(pg_pool.clone()));
    let vram_pool: Arc<dyn VramPoolPort> = if let Some(pool) = valkey_pool {
        Arc::new(
            veronex::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool::new(
                pool.clone(),
                instance_id.clone(),
            ),
        )
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
                vram_pool.set_baseline_p95_ms(
                    p.provider_id,
                    &p.model_name,
                    p.baseline_p95_ms as u32,
                );
            }
        }
        if !profiles.is_empty() {
            tracing::info!(count = profiles.len(), "restored AIMD profiles from DB");
        }
    }

    let vram_budget_repo: Arc<dyn ProviderVramBudgetRepository> =
        Arc::new(PostgresProviderVramBudgetRepository::new(pg_pool.clone()));

    Ok(Repositories {
        account_repo,
        api_key_repo,
        job_repo,
        provider_registry,
        gpu_server_registry,
        gemini_policy_repo,
        model_selection_repo,
        gemini_sync_config_repo,
        gemini_model_repo,
        ollama_model_repo,
        ollama_sync_job_repo,
        session_repo,
        lab_settings_repo,
        capacity_repo,
        capacity_settings_repo,
        valkey_port,
        observability,
        audit_port,
        analytics_repo,
        model_manager,
        vram_pool,
        message_store,
        image_store,
        vram_budget_repo,
    })
}

/// Bootstrap super account if BOOTSTRAP_SUPER_USER + BOOTSTRAP_SUPER_PASS are set.
pub async fn maybe_bootstrap_super_account(
    account_repo: &Arc<dyn AccountRepository>,
    config: &AppConfig,
    pg_pool: &sqlx::PgPool,
) {
    let (user, pass) = match (&config.bootstrap_super_user, &config.bootstrap_super_pass) {
        (Some(u), Some(p)) => (u.clone(), p.clone()),
        _ => return,
    };
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
                    // Look up the super role_id from seeded roles table.
                    let super_role_id = match sqlx::query_as::<_, (uuid::Uuid,)>("SELECT id FROM roles WHERE name = 'super'")
                        .fetch_optional(pg_pool)
                        .await
                    {
                        Ok(Some(row)) => row.0,
                        Ok(None) => { tracing::warn!("super role not found in DB — skip bootstrap"); return; }
                        Err(e) => { tracing::warn!("failed to query super role: {e}"); return; }
                    };
                    let super_account = veronex::domain::entities::Account {
                        id: uuid::Uuid::now_v7(),
                        username: user.clone(),
                        password_hash: hash,
                        name: "Super Admin".to_string(),
                        email: None,
                        department: None,
                        position: None,
                        is_active: true,
                        created_by: None,
                        last_login_at: None,
                        created_at: chrono::Utc::now(),
                        deleted_at: None,
                    };
                    match account_repo.create_with_roles(&super_account, &[super_role_id]).await {
                        Ok(()) => tracing::info!("bootstrap super account '{user}' created"),
                        Err(e) => {
                            tracing::warn!("failed to create bootstrap super account: {e}")
                        }
                    }
                }
                Err(e) => tracing::warn!("failed to hash bootstrap super password: {e}"),
            }
        }
        Err(e) => tracing::warn!("failed to check bootstrap super account: {e}"),
    }
}
