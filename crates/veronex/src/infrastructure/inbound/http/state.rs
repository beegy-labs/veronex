use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::sync::atomic::AtomicU32;

use dashmap::DashMap;

use tokio::sync::{broadcast, Notify};
use uuid::Uuid;

use crate::domain::value_objects::{FlowStats, JobStatusEvent};

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::application::ports::outbound::account_repository::AccountRepository;
use crate::application::ports::outbound::image_store::ImageStore;
use crate::application::ports::outbound::message_store::MessageStore;
use crate::application::ports::outbound::analytics_repository::AnalyticsRepository;
use crate::application::ports::outbound::api_key_repository::ApiKeyRepository;
use crate::application::ports::outbound::audit_port::AuditPort;
use crate::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use crate::application::ports::outbound::global_model_settings::GlobalModelSettingsRepository;
use crate::application::ports::outbound::api_key_provider_access::ApiKeyProviderAccessRepository;
use crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository;
use crate::application::ports::outbound::lab_settings_repository::LabSettingsRepository;
use crate::application::ports::outbound::mcp_settings_repository::McpSettingsRepository;
use crate::application::ports::outbound::gemini_repository::GeminiModelRepository;
use crate::application::ports::outbound::gemini_repository::GeminiPolicyRepository;
use crate::application::ports::outbound::gemini_repository::GeminiSyncConfigRepository;
use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::model_capacity_repository::ModelCapacityRepository;
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::application::ports::outbound::ollama_sync_job_repository::OllamaSyncJobRepository;
use crate::application::ports::outbound::session_repository::SessionRepository;
use crate::application::ports::outbound::concurrency_port::VramPoolPort;
use crate::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use crate::infrastructure::outbound::circuit_breaker::CircuitBreakerMap;
use crate::infrastructure::outbound::hw_metrics::CpuSnapshot;
use crate::infrastructure::outbound::mcp::McpBridgeAdapter;
use veronex_mcp::vector::{McpToolIndexer, McpVectorSelector};

/// Shared application state passed to all HTTP handlers via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    /// Shared `reqwest::Client` — reuse across handlers and background tasks.
    /// `reqwest::Client` is `Clone + Arc` internally; cloning is cheap.
    pub http_client: reqwest::Client,
    pub use_case: Arc<dyn InferenceUseCase>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub account_repo: Arc<dyn AccountRepository>,
    pub audit_port: Option<Arc<dyn AuditPort>>,
    pub jwt_secret: String,
    pub provider_registry: Arc<dyn LlmProviderRegistry>,
    pub gpu_server_registry: Arc<dyn GpuServerRegistry>,
    pub gemini_policy_repo: Arc<dyn GeminiPolicyRepository>,
    pub gemini_sync_config_repo: Arc<dyn GeminiSyncConfigRepository>,
    pub gemini_model_repo: Arc<dyn GeminiModelRepository>,
    pub model_selection_repo: Arc<dyn ProviderModelSelectionRepository>,
    pub global_model_settings_repo: Arc<dyn GlobalModelSettingsRepository>,
    pub api_key_provider_access_repo: Arc<dyn ApiKeyProviderAccessRepository>,
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
    // ── VRAM pool + thermal ──────────────────────────────────────────
    /// Per-provider VRAM pool — updated by sync loop.
    pub vram_pool: Arc<dyn VramPoolPort>,
    /// Thermal throttle state — updated by health_checker.
    pub thermal: Arc<ThermalThrottleMap>,
    /// Capacity analysis results — persisted VRAM + throughput stats.
    pub capacity_repo: Arc<dyn ModelCapacityRepository>,
    /// Capacity analysis settings (singleton row in DB).
    pub capacity_settings_repo: Arc<dyn CapacitySettingsRepository>,
    /// Fire to trigger an immediate sync run (bypasses sync interval).
    pub sync_trigger: Arc<Notify>,
    /// Ollama URL used by the capacity analyzer (CAPACITY_ANALYZER_OLLAMA_URL).
    pub analyzer_url: String,
    /// Broadcast channel sender for real-time job status events.
    /// Handlers subscribe by calling `.subscribe()` on this sender.
    pub job_event_tx: Arc<broadcast::Sender<JobStatusEvent>>,
    /// Rolling replay buffer of the last 100 job events, with server-side timestamps.
    /// Sent to every new SSE client on connect so all users see the same view
    /// regardless of when they connected.
    /// Each entry is `(event, unix_ms)` — the unix_ms is used by the stats ticker.
    pub event_ring_buffer: Arc<RwLock<VecDeque<(JobStatusEvent, u64)>>>,
    /// Broadcast channel for real-time aggregate stats (incoming/queued/running/completed).
    /// Pushed every second by the stats ticker; all SSE clients receive the same values.
    pub stats_tx: Arc<broadcast::Sender<FlowStats>>,
    /// Lab (experimental) feature flags — singleton row in DB.
    pub lab_settings_repo: Arc<dyn LabSettingsRepository>,
    /// MCP global settings — singleton row in DB.
    pub mcp_settings_repo: Arc<dyn McpSettingsRepository>,
    /// Per-provider circuit breaker — isolates failing providers automatically.
    pub circuit_breaker: Arc<CircuitBreakerMap>,
    /// S3-compatible object store for conversation contexts (messages_json).
    /// `None` when S3_ENDPOINT is not configured (messages stay in PostgreSQL).
    pub message_store: Option<Arc<dyn MessageStore>>,
    /// S3-compatible object store for inference job images (WebP).
    /// `None` when S3_ENDPOINT is not configured.
    pub image_store: Option<Arc<dyn ImageStore>>,
    /// Mutex (1-permit semaphore) that prevents concurrent session grouping runs.
    /// Held for the duration of each run — manual trigger returns 409 if locked.
    pub session_grouping_lock: Arc<tokio::sync::Semaphore>,
    /// Mutex (1-permit semaphore) that prevents concurrent sync runs.
    /// Held for the duration of each run — POST /sync returns 409 if locked.
    pub sync_lock: Arc<tokio::sync::Semaphore>,
    /// Global concurrent SSE connection counter.
    /// Prevents resource exhaustion from too many open SSE streams.
    pub sse_connections: Arc<AtomicU32>,
    /// Per-API-key in-flight semaphore (Slowloris defense).
    /// Keyed by API key UUID; `try_acquire_owned()` immediately 429s when limit hit.
    pub key_in_flight: Arc<DashMap<Uuid, Arc<tokio::sync::Semaphore>>>,
    /// Persistent VRAM budget state per provider (safety_permil, source, kv_cache_type).
    pub vram_budget_repo: Arc<dyn crate::application::ports::outbound::provider_vram_budget_repository::ProviderVramBudgetRepository>,
    /// MCP bridge adapter — present when at least one MCP server is configured.
    /// `None` disables MCP tool injection on all requests.
    pub mcp_bridge: Option<Arc<McpBridgeAdapter>>,
    /// Vespa-backed vector selector for MCP tool selection.
    /// `None` when VESPA_URL is not configured — falls back to get_all().
    pub mcp_vector_selector: Option<Arc<McpVectorSelector>>,
    /// Tool indexer — embeds and feeds tools to Vespa on server register/delete.
    /// `None` when VESPA_URL is not configured.
    pub mcp_tool_indexer: Option<Arc<McpToolIndexer>>,
    /// Deployment-level Vespa partition key — from `VESPA_DEPLOYMENT_ID` env var.
    /// Isolates this deployment's documents from others on a shared Vespa instance.
    pub vespa_deployment_id: Arc<str>,
    /// Instance ID of this API pod (UUID string).
    /// Used by service health endpoint to identify pods.
    pub instance_id: Arc<str>,
    /// Maximum login attempts per IP per 5-minute window.
    /// `0` disables IP-based rate limiting (e.g. for E2E test environments).
    /// Controlled via `LOGIN_RATE_LIMIT` env var (default: 10).
    pub login_rate_limit: u64,
    /// Redpanda metrics URL for high-watermark scraping (e.g. `http://redpanda:9644`).
    pub kafka_broker_admin_url: Option<Arc<str>>,
    /// ClickHouse HTTP base URL for pipeline stats queries.
    pub clickhouse_http_url: Option<Arc<str>>,
    pub clickhouse_user: Option<Arc<str>>,
    pub clickhouse_password: Option<Arc<str>>,
    pub clickhouse_db: Option<Arc<str>>,
}
