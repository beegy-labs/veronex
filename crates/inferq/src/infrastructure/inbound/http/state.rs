use std::sync::Arc;

use dashmap::DashMap;

use tokio::sync::{broadcast, Notify};
use uuid::Uuid;

use crate::domain::value_objects::JobStatusEvent;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::account_repository::AccountRepository;
use crate::application::ports::outbound::message_store::MessageStore;
use crate::application::ports::outbound::analytics_repository::AnalyticsRepository;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::application::ports::outbound::audit_port::AuditPort;
use crate::application::ports::outbound::backend_model_selection::BackendModelSelectionRepository;
use crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository;
use crate::application::ports::outbound::lab_settings_repository::LabSettingsRepository;
use crate::application::ports::outbound::gemini_model_repository::GeminiModelRepository;
use crate::application::ports::outbound::gemini_policy_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::gemini_sync_config_repository::GeminiSyncConfigRepository;
use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::application::ports::outbound::model_capacity_repository::ModelCapacityRepository;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::application::ports::outbound::ollama_sync_job_repository::OllamaSyncJobRepository;
use crate::application::ports::outbound::session_repository::SessionRepository;
use crate::infrastructure::outbound::capacity::slot_map::ConcurrencySlotMap;
use crate::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use crate::infrastructure::outbound::circuit_breaker::CircuitBreakerMap;
use crate::infrastructure::outbound::hw_metrics::CpuSnapshot;

/// Shared application state passed to all HTTP handlers via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    pub use_case: Arc<dyn InferenceUseCase>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub account_repo: Arc<dyn AccountRepository>,
    pub audit_port: Option<Arc<dyn AuditPort>>,
    pub jwt_secret: String,
    pub backend_registry: Arc<dyn LlmBackendRegistry>,
    pub gpu_server_registry: Arc<dyn GpuServerRegistry>,
    pub gemini_policy_repo: Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo: Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo: Arc<dyn GeminiModelRepository>,
    pub model_selection_repo: Arc<dyn BackendModelSelectionRepository>,
    pub ollama_model_repo: Arc<dyn OllamaModelRepository>,
    pub ollama_sync_job_repo: Arc<dyn OllamaSyncJobRepository>,
    pub valkey_pool: Option<fred::clients::Pool>,
    /// Analytics repository — proxies queries through veronex-analytics service.
    /// `None` when ANALYTICS_URL is not configured.
    pub analytics_repo: Option<Arc<dyn AnalyticsRepository>>,
    pub session_repo: Arc<dyn SessionRepository>,
    pub pg_pool: sqlx::PgPool,
    /// Per-server CPU counter snapshots for delta-based usage calculation.
    /// Keyed by GpuServer ID; updated on every metrics scrape.
    pub cpu_snapshot_cache: Arc<DashMap<Uuid, CpuSnapshot>>,
    // ── Dynamic concurrency + thermal ──────────────────────────────
    /// Per-(backend, model) concurrency semaphores — updated by capacity analyzer.
    pub slot_map: Arc<ConcurrencySlotMap>,
    /// Thermal throttle state — updated by health_checker.
    pub thermal: Arc<ThermalThrottleMap>,
    /// Capacity analysis results — persisted VRAM + throughput stats.
    pub capacity_repo: Arc<dyn ModelCapacityRepository>,
    /// Capacity analysis settings (singleton row in DB).
    pub capacity_settings_repo: Arc<dyn CapacitySettingsRepository>,
    /// Fire to trigger an immediate capacity analysis run (bypasses batch interval).
    pub capacity_manual_trigger: Arc<Notify>,
    /// Ollama URL used by the capacity analyzer (CAPACITY_ANALYZER_OLLAMA_URL).
    pub analyzer_url: String,
    /// Broadcast channel sender for real-time job status events.
    /// Handlers subscribe by calling `.subscribe()` on this sender.
    pub job_event_tx: Arc<broadcast::Sender<JobStatusEvent>>,
    /// Lab (experimental) feature flags — singleton row in DB.
    pub lab_settings_repo: Arc<dyn LabSettingsRepository>,
    /// Per-backend circuit breaker — isolates failing backends automatically.
    pub circuit_breaker: Arc<CircuitBreakerMap>,
    /// S3-compatible object store for conversation contexts (messages_json).
    /// `None` when S3_ENDPOINT is not configured (messages stay in PostgreSQL).
    pub message_store: Option<Arc<dyn MessageStore>>,
    /// Mutex (1-permit semaphore) that prevents concurrent session grouping runs.
    /// Held for the duration of each run — manual trigger returns 409 if locked.
    pub session_grouping_lock: Arc<tokio::sync::Semaphore>,
}
