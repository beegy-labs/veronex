use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use crate::application::ports::inbound::inference_use_case::{InferenceUseCase, SubmitJobRequest};
use crate::application::ports::outbound::account_repository::AccountRepository;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::application::ports::outbound::lab_settings_repository::{LabSettings, LabSettingsRepository};
use crate::application::ports::outbound::mcp_settings_repository::{McpSettings, McpSettingsRepository};
use crate::application::ports::outbound::provider_model_selection::{ProviderModelSelectionRepository, ProviderSelectedModel};
use crate::application::ports::outbound::gemini_repository::{GeminiModel, GeminiModelRepository};
use crate::application::ports::outbound::gemini_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::gemini_repository::GeminiSyncConfigRepository;
use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::ollama_model_repository::{ModelPage, OllamaModelRepository, OllamaModelWithCount, ProviderPage};
use crate::application::ports::outbound::ollama_sync_job_repository::{OllamaSyncJob, OllamaSyncJobRepository};
use crate::application::ports::outbound::provider_vram_budget_repository::{ProviderVramBudget, ProviderVramBudgetRepository};
use crate::application::ports::outbound::session_repository::SessionRepository;
use crate::domain::entities::{Account, ApiKey, GeminiRateLimitPolicy, GpuServer, LlmProvider, Session};
use crate::infrastructure::inbound::http::middleware::infer_auth::InferCaller;
use crate::domain::enums::{JobStatus, KeyTier, KeyType, LlmProviderStatus};
use crate::domain::errors::DomainError;
use crate::domain::value_objects::{JobId, StreamToken};
use crate::infrastructure::inbound::http::router;
use crate::infrastructure::inbound::http::state::AppState;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use uuid::Uuid;

// ── Mock InferenceUseCase for handler tests ────────────────────

pub(crate) struct MockUseCase;

#[async_trait]
impl InferenceUseCase for MockUseCase {
    async fn submit(&self, _req: SubmitJobRequest) -> std::result::Result<JobId, DomainError> {
        Ok(JobId::new())
    }

    async fn process(&self, _job_id: &JobId) -> std::result::Result<(), DomainError> {
        Ok(())
    }

    fn stream(
        &self,
        _job_id: &JobId,
    ) -> Pin<Box<dyn Stream<Item = std::result::Result<StreamToken, DomainError>> + Send>> {
        let tokens = vec![
            Ok(StreamToken {
                value: "Hello".to_string(),
                is_final: false,
                prompt_tokens: None,
                completion_tokens: None,
                cached_tokens: None,
                tool_calls: None,
                finish_reason: None,
            }),
            Ok(StreamToken {
                value: "".to_string(),
                is_final: true,
                prompt_tokens: None,
                completion_tokens: None,
                cached_tokens: None,
                tool_calls: None,
                finish_reason: None,
            }),
        ];
        Box::pin(futures::stream::iter(tokens))
    }

    async fn get_status(&self, _job_id: &JobId) -> std::result::Result<JobStatus, DomainError> {
        Ok(JobStatus::Running)
    }

    async fn cancel(&self, _job_id: &JobId) -> std::result::Result<(), DomainError> {
        Ok(())
    }

    fn get_live_counts(&self) -> crate::application::ports::inbound::inference_use_case::LiveCounts {
        crate::application::ports::inbound::inference_use_case::LiveCounts { pending: 0, running: 0 }
    }
}

pub(crate) struct MockApiKeyRepo;

#[async_trait]
impl ApiKeyRepository for MockApiKeyRepo {
    async fn create(&self, _key: &ApiKey) -> Result<()> {
        Ok(())
    }
    async fn get_by_id(&self, _key_id: &Uuid) -> Result<Option<ApiKey>> {
        Ok(None)
    }
    async fn get_by_hash(&self, _key_hash: &str) -> Result<Option<ApiKey>> {
        Ok(None)
    }
    async fn list_by_tenant(&self, _tenant_id: &str) -> Result<Vec<ApiKey>> {
        Ok(vec![])
    }
    async fn list_all(&self) -> Result<Vec<ApiKey>> {
        Ok(vec![])
    }
    async fn list_page(&self, _search: &str, _limit: i64, _offset: i64) -> Result<(Vec<ApiKey>, i64)> {
        Ok((vec![], 0))
    }
    async fn list_by_tenant_page(&self, _tenant_id: &str, _search: &str, _limit: i64, _offset: i64) -> Result<(Vec<ApiKey>, i64)> {
        Ok((vec![], 0))
    }
    async fn revoke(&self, _key_id: &Uuid) -> Result<()> {
        Ok(())
    }
    async fn set_active(&self, _key_id: &Uuid, _active: bool) -> Result<()> {
        Ok(())
    }
    async fn soft_delete(&self, _key_id: &Uuid) -> Result<()> {
        Ok(())
    }
    async fn set_tier(&self, _key_id: &Uuid, _tier: &KeyTier) -> Result<()> {
        Ok(())
    }
    async fn update_fields(&self, _key_id: &Uuid, _is_active: Option<bool>, _tier: Option<&KeyTier>) -> Result<()> {
        Ok(())
    }
    async fn soft_delete_by_tenant(&self, _tenant_id: &str) -> Result<u64> {
        Ok(0)
    }
    async fn regenerate(&self, _key_id: &Uuid, _new_hash: &str, _new_prefix: &str) -> Result<()> {
        Ok(())
    }
}

pub(crate) struct MockProviderRegistry;

#[async_trait]
impl LlmProviderRegistry for MockProviderRegistry {
    async fn register(&self, _provider: &LlmProvider) -> Result<()> { Ok(()) }
    async fn list_active(&self) -> Result<Vec<LlmProvider>> { Ok(vec![]) }
    async fn list_all(&self) -> Result<Vec<LlmProvider>> { Ok(vec![]) }
    async fn list_page(&self, _search: &str, _provider_type: Option<&str>, _limit: i64, _offset: i64) -> Result<(Vec<LlmProvider>, i64)> { Ok((vec![], 0)) }
    async fn get(&self, _id: Uuid) -> Result<Option<LlmProvider>> { Ok(None) }
    async fn update_status(&self, _id: Uuid, _status: LlmProviderStatus) -> Result<()> { Ok(()) }
    async fn deactivate(&self, _id: Uuid) -> Result<()> { Ok(()) }
    async fn update(&self, _provider: &LlmProvider) -> Result<()> { Ok(()) }
}

pub(crate) struct MockGpuServerRegistry;

#[async_trait]
impl GpuServerRegistry for MockGpuServerRegistry {
    async fn register(&self, _server: GpuServer) -> Result<()> { Ok(()) }
    async fn list_all(&self) -> Result<Vec<GpuServer>> { Ok(vec![]) }
    async fn list_page(&self, _search: &str, _limit: i64, _offset: i64) -> Result<(Vec<GpuServer>, i64)> { Ok((vec![], 0)) }
    async fn get(&self, _id: Uuid) -> Result<Option<GpuServer>> { Ok(None) }
    async fn update(&self, _server: &GpuServer) -> Result<()> { Ok(()) }
    async fn delete(&self, _id: Uuid) -> Result<()> { Ok(()) }
}

pub(crate) struct MockGeminiPolicyRepo;

#[async_trait]
impl GeminiPolicyRepository for MockGeminiPolicyRepo {
    async fn list_all(&self) -> Result<Vec<GeminiRateLimitPolicy>> { Ok(vec![]) }
    async fn get_for_model(&self, _model_name: &str) -> Result<Option<GeminiRateLimitPolicy>> { Ok(None) }
    async fn upsert(&self, _policy: &GeminiRateLimitPolicy) -> Result<()> { Ok(()) }
}

pub(crate) struct MockGeminiSyncConfigRepo;

#[async_trait]
impl GeminiSyncConfigRepository for MockGeminiSyncConfigRepo {
    async fn get_api_key(&self) -> Result<Option<String>> { Ok(None) }
    async fn set_api_key(&self, _api_key: &str) -> Result<()> { Ok(()) }
}

pub(crate) struct MockGeminiModelRepo;

#[async_trait]
impl GeminiModelRepository for MockGeminiModelRepo {
    async fn sync_models(&self, _model_names: &[String]) -> Result<()> { Ok(()) }
    async fn list(&self) -> Result<Vec<GeminiModel>> { Ok(vec![]) }
}

pub(crate) struct MockModelSelectionRepo;

#[async_trait]
impl ProviderModelSelectionRepository for MockModelSelectionRepo {
    async fn upsert_models(&self, _provider_id: Uuid, _models: &[String]) -> Result<()> { Ok(()) }
    async fn list(&self, _provider_id: Uuid) -> Result<Vec<ProviderSelectedModel>> { Ok(vec![]) }
    async fn set_enabled(&self, _provider_id: Uuid, _model_name: &str, _enabled: bool) -> Result<()> { Ok(()) }
    async fn list_enabled(&self, _provider_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
    async fn list_disabled(&self, _provider_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
}

pub(crate) struct MockGlobalModelSettingsRepo;

#[async_trait]
impl crate::application::ports::outbound::global_model_settings::GlobalModelSettingsRepository for MockGlobalModelSettingsRepo {
    async fn list(&self) -> Result<Vec<crate::application::ports::outbound::global_model_settings::GlobalModelSetting>> { Ok(vec![]) }
    async fn is_enabled(&self, _model_name: &str) -> Result<bool> { Ok(true) }
    async fn set_enabled(&self, _model_name: &str, _enabled: bool) -> Result<()> { Ok(()) }
    async fn list_disabled(&self) -> Result<Vec<String>> { Ok(vec![]) }
}

pub(crate) struct MockApiKeyProviderAccessRepo;

#[async_trait]
impl crate::application::ports::outbound::api_key_provider_access::ApiKeyProviderAccessRepository for MockApiKeyProviderAccessRepo {
    async fn list_allowed(&self, _api_key_id: Uuid) -> Result<Vec<Uuid>> { Ok(vec![]) }
    async fn set_access(&self, _api_key_id: Uuid, _provider_id: Uuid, _allowed: bool) -> Result<()> { Ok(()) }
    async fn list(&self, _api_key_id: Uuid) -> Result<Vec<(Uuid, bool)>> { Ok(vec![]) }
    async fn has_restrictions(&self, _api_key_id: Uuid) -> Result<bool> { Ok(false) }
}

pub(crate) struct MockOllamaModelRepo;

#[async_trait]
impl OllamaModelRepository for MockOllamaModelRepo {
    async fn sync_provider_models(&self, _provider_id: Uuid, _model_names: &[String]) -> Result<()> { Ok(()) }
    async fn list_all(&self) -> Result<Vec<String>> { Ok(vec![]) }
    async fn list_with_counts(&self) -> Result<Vec<OllamaModelWithCount>> { Ok(vec![]) }
    async fn list_with_counts_page(&self, _search: &str, _limit: i64, _offset: i64) -> Result<ModelPage> {
        Ok(ModelPage { items: vec![], total: 0 })
    }
    async fn providers_for_model(&self, _model_name: &str) -> Result<Vec<Uuid>> { Ok(vec![]) }
    async fn providers_info_for_model_page(&self, _model_name: &str, _search: &str, _limit: i64, _offset: i64) -> Result<ProviderPage> {
        Ok(ProviderPage { items: vec![], total: 0 })
    }
    async fn models_for_provider(&self, _provider_id: Uuid) -> Result<Vec<String>> { Ok(vec![]) }
}

pub(crate) struct MockOllamaSyncJobRepo;

#[async_trait]
impl OllamaSyncJobRepository for MockOllamaSyncJobRepo {
    async fn create(&self, _total_providers: i32) -> Result<Uuid> { Ok(Uuid::now_v7()) }
    async fn update_progress(&self, _id: Uuid, _result: serde_json::Value) -> Result<()> { Ok(()) }
    async fn complete(&self, _id: Uuid) -> Result<()> { Ok(()) }
    async fn get_latest(&self) -> Result<Option<OllamaSyncJob>> { Ok(None) }
}

pub(crate) struct MockAccountRepo;

#[async_trait]
impl AccountRepository for MockAccountRepo {
    async fn create(&self, _account: &Account) -> Result<()> { Ok(()) }
    async fn get_by_id(&self, _id: &Uuid) -> Result<Option<Account>> { Ok(None) }
    async fn get_by_username(&self, _username: &str) -> Result<Option<Account>> { Ok(None) }
    async fn list_all(&self) -> Result<Vec<Account>> { Ok(vec![]) }
    async fn list_page(&self, _search: &str, _limit: i64, _offset: i64) -> Result<(Vec<Account>, i64)> { Ok((vec![], 0)) }
    async fn update(&self, _account: &Account) -> Result<()> { Ok(()) }
    async fn soft_delete(&self, _id: &Uuid) -> Result<()> { Ok(()) }
    async fn soft_delete_cascade(&self, _account_id: &Uuid, _tenant_id: &str) -> Result<u64> { Ok(0) }
    async fn set_active(&self, _id: &Uuid, _is_active: bool) -> Result<()> { Ok(()) }
    async fn update_last_login(&self, _id: &Uuid) -> Result<()> { Ok(()) }
    async fn set_password_hash(&self, _id: &Uuid, _hash: &str) -> Result<()> { Ok(()) }
    async fn create_with_roles(&self, _account: &Account, _role_ids: &[Uuid]) -> Result<()> { Ok(()) }
    async fn set_roles(&self, _account_id: &Uuid, _role_ids: &[Uuid]) -> Result<()> { Ok(()) }
    async fn get_role_ids(&self, _account_id: &Uuid) -> Result<Vec<Uuid>> { Ok(vec![]) }
}

pub(crate) struct MockCapacityRepo;

#[async_trait]
impl crate::application::ports::outbound::model_capacity_repository::ModelCapacityRepository for MockCapacityRepo {
    async fn upsert(&self, _: &crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry) -> Result<()> { Ok(()) }
    async fn get(&self, _: uuid::Uuid, _: &str) -> Result<Option<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(None) }
    async fn list_all(&self) -> Result<Vec<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(vec![]) }
    async fn list_by_provider(&self, _: uuid::Uuid) -> Result<Vec<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(vec![]) }
    async fn list_by_providers(&self, _: &[uuid::Uuid]) -> Result<Vec<crate::application::ports::outbound::model_capacity_repository::ModelVramProfileEntry>> { Ok(vec![]) }
    async fn compute_throughput_stats(&self, _: uuid::Uuid, _: &str, _: u32) -> Result<Option<crate::application::ports::outbound::model_capacity_repository::ThroughputStats>> { Ok(None) }
}

pub(crate) struct MockCapacitySettingsRepo;

#[async_trait]
impl crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository for MockCapacitySettingsRepo {
    async fn get(&self) -> Result<crate::application::ports::outbound::capacity_settings_repository::CapacitySettings> {
        Ok(crate::application::ports::outbound::capacity_settings_repository::CapacitySettings::default())
    }
    async fn update_settings(&self, _: Option<&str>, _: Option<bool>, _: Option<i32>, _: Option<i32>, _: Option<i32>) -> Result<crate::application::ports::outbound::capacity_settings_repository::CapacitySettings> {
        Ok(crate::application::ports::outbound::capacity_settings_repository::CapacitySettings::default())
    }
    async fn record_run(&self, _: &str) -> Result<()> { Ok(()) }
}

pub(crate) struct MockMcpSettingsRepo;

#[async_trait]
impl McpSettingsRepository for MockMcpSettingsRepo {
    async fn get(&self) -> anyhow::Result<McpSettings> { Ok(McpSettings::default()) }
    async fn update(&self, _patch: crate::application::ports::outbound::mcp_settings_repository::McpSettingsUpdate) -> anyhow::Result<McpSettings> { Ok(McpSettings::default()) }
}

pub(crate) struct MockLabSettingsRepo;

#[async_trait]
impl LabSettingsRepository for MockLabSettingsRepo {
    async fn get(&self) -> Result<LabSettings> {
        Ok(LabSettings::default())
    }
    async fn update(&self, _patch: crate::application::ports::outbound::lab_settings_repository::LabSettingsUpdate) -> Result<LabSettings> {
        Ok(LabSettings::default())
    }
}

pub(crate) struct MockSessionRepo;

#[async_trait]
impl SessionRepository for MockSessionRepo {
    async fn create(&self, _session: &Session) -> Result<()> { Ok(()) }
    async fn list_active(&self, _account_id: &Uuid) -> Result<Vec<Session>> { Ok(vec![]) }
    async fn get_by_refresh_hash(&self, _hash: &str) -> Result<Option<Session>> { Ok(None) }
    async fn revoke(&self, _session_id: &Uuid) -> Result<()> { Ok(()) }
    async fn get_by_id(&self, _session_id: &Uuid) -> Result<Option<Session>> { Ok(None) }
    async fn revoke_all_for_account(&self, _account_id: &Uuid) -> Result<()> { Ok(()) }
    async fn update_last_used(&self, _jti: &Uuid) -> Result<()> { Ok(()) }
}

pub(crate) struct MockVramBudgetRepo;

#[async_trait]
impl ProviderVramBudgetRepository for MockVramBudgetRepo {
    async fn get(&self, _provider_id: Uuid) -> Result<Option<ProviderVramBudget>> { Ok(None) }
    async fn upsert(&self, _budget: &ProviderVramBudget) -> Result<()> { Ok(()) }
}

pub(crate) fn make_app() -> axum::Router {
    let fake_key = ApiKey {
        id: Uuid::now_v7(),
        key_hash: "testhash".to_string(),
        key_prefix: "vnx_test".to_string(),
        tenant_id: "test-tenant".to_string(),
        name: "test-key".to_string(),
        is_active: true,
        rate_limit_rpm: 0,
        rate_limit_tpm: 0,
        expires_at: None,
        deleted_at: None,
        created_at: chrono::Utc::now(),
        key_type: KeyType::Standard,
        tier: KeyTier::Paid,
        mcp_cap_points: 3,
        account_id: None,
    };
    let pg_pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://test:test@localhost/test")
        .expect("lazy pool creation should not fail");
    let state = AppState {
        http_client: reqwest::Client::new(),
        use_case: Arc::new(MockUseCase),
        api_key_repo: Arc::new(MockApiKeyRepo),
        account_repo: Arc::new(MockAccountRepo),
        audit_port: None,
        jwt_secret: "test-secret".to_string(),
        provider_registry: Arc::new(MockProviderRegistry),
        gpu_server_registry: Arc::new(MockGpuServerRegistry),
        gemini_policy_repo: Arc::new(MockGeminiPolicyRepo),
        gemini_sync_config_repo: Arc::new(MockGeminiSyncConfigRepo),
        gemini_model_repo: Arc::new(MockGeminiModelRepo),
        model_selection_repo: Arc::new(MockModelSelectionRepo),
        global_model_settings_repo: Arc::new(MockGlobalModelSettingsRepo),
        api_key_provider_access_repo: Arc::new(MockApiKeyProviderAccessRepo),
        ollama_model_repo: Arc::new(MockOllamaModelRepo),
        ollama_sync_job_repo: Arc::new(MockOllamaSyncJobRepo),
        valkey_pool: None,
        analytics_repo: None,
        session_repo: Arc::new(MockSessionRepo),
        pg_pool,
        cpu_snapshot_cache: Arc::new(dashmap::DashMap::new()),
        vram_pool: Arc::new(crate::infrastructure::outbound::capacity::vram_pool::VramPool::new()) as Arc<dyn crate::application::ports::outbound::concurrency_port::VramPoolPort>,
        thermal: Arc::new(crate::infrastructure::outbound::capacity::thermal::ThermalThrottleMap::new(300)),
        capacity_repo: Arc::new(MockCapacityRepo),
        capacity_settings_repo: Arc::new(MockCapacitySettingsRepo),
        sync_trigger: Arc::new(tokio::sync::Notify::new()),
        analyzer_url: String::new(),
        job_event_tx: Arc::new(tokio::sync::broadcast::channel(1).0),
        event_ring_buffer: Arc::new(std::sync::RwLock::new(std::collections::VecDeque::new())),
        stats_tx: Arc::new(tokio::sync::broadcast::channel(1).0),
        circuit_breaker: Arc::new(crate::infrastructure::outbound::circuit_breaker::CircuitBreakerMap::new()),
        message_store: None,
        image_store: None,
        session_grouping_lock: Arc::new(tokio::sync::Semaphore::new(1)),
        sync_lock: Arc::new(tokio::sync::Semaphore::new(1)),
        lab_settings_repo: Arc::new(MockLabSettingsRepo),
        mcp_settings_repo: Arc::new(MockMcpSettingsRepo),
        sse_connections: Arc::new(AtomicU32::new(0)),
        key_in_flight: Arc::new(dashmap::DashMap::new()),
        vram_budget_repo: Arc::new(MockVramBudgetRepo),
        mcp_bridge: None,
        mcp_vector_selector: None,
        mcp_tool_indexer: None,
        instance_id: Arc::from("test-instance"),
        login_rate_limit: 0,
        kafka_broker_admin_url: None,
        clickhouse_http_url: None,
        clickhouse_user: None,
        clickhouse_password: None,
        clickhouse_db: None,
        vespa_environment: Arc::from(""),
        vespa_tenant_id: Arc::from(""),
    };
    // Inject a fake InferCaller extension so handlers that extract it work in tests.
    router::build_api_router()
        .layer(axum::Extension(InferCaller::ApiKey(fake_key)))
        .with_state(state)
}
