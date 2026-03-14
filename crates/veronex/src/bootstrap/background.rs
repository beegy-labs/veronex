use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use veronex::application::ports::inbound::inference_use_case::InferenceUseCase;
use veronex::application::use_cases::InferenceUseCaseImpl;
use veronex::domain::value_objects::JobStatusEvent;
use veronex::infrastructure::outbound::capacity::analyzer::run_sync_loop;
use veronex::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use veronex::infrastructure::outbound::circuit_breaker::CircuitBreakerMap;
use veronex::infrastructure::outbound::health_checker::run_health_checker_loop;
use veronex::infrastructure::outbound::provider_dispatch::ConcreteProviderDispatch;
use veronex::infrastructure::outbound::session_grouping::run_session_grouping_loop;

use super::config::AppConfig;
use super::repositories::Repositories;

/// Infrastructure context shared between repository wiring and background tasks.
pub struct InfraContext {
    pub valkey_pool: Option<fred::clients::Pool>,
    pub pg_pool: sqlx::PgPool,
    pub http_client: reqwest::Client,
    pub instance_id: Arc<str>,
}

/// All shared infrastructure handles created during background task setup.
/// Returned so `main` can wire them into `AppState`.
pub struct BackgroundHandles {
    pub thermal: Arc<ThermalThrottleMap>,
    pub circuit_breaker: Arc<CircuitBreakerMap>,
    pub sync_trigger: Arc<tokio::sync::Notify>,
    pub sync_lock: Arc<tokio::sync::Semaphore>,
    pub session_grouping_lock: Arc<tokio::sync::Semaphore>,
    pub job_event_tx: Arc<tokio::sync::broadcast::Sender<JobStatusEvent>>,
    pub use_case: Arc<dyn InferenceUseCase>,
}

/// Spawn all background tasks and return shared handles for AppState.
pub async fn spawn_background_tasks(
    repos: &Repositories,
    config: &AppConfig,
    infra: &InfraContext,
    shutdown: &CancellationToken,
    tasks: &mut JoinSet<()>,
) -> BackgroundHandles {
    let thermal = Arc::new(ThermalThrottleMap::new(300)); // 300s cooldown (SDD §3)
    let circuit_breaker = Arc::new(CircuitBreakerMap::new());
    let sync_trigger = Arc::new(tokio::sync::Notify::new());
    let sync_lock = Arc::new(tokio::sync::Semaphore::new(1));

    // ── Health checker ─────────────────────────────────────────────
    tasks.spawn(run_health_checker_loop(
        repos.provider_registry.clone(),
        repos.gpu_server_registry.clone(),
        30,
        infra.valkey_pool.clone(),
        thermal.clone(),
        shutdown.child_token(),
        infra.http_client.clone(),
        repos.vram_pool.clone(),
    ));

    // ── Sync loop (unified: health + models + VRAM) ────────────────
    tasks.spawn(run_sync_loop(
        repos.provider_registry.clone(),
        repos.capacity_repo.clone(),
        repos.capacity_settings_repo.clone(),
        repos.vram_pool.clone(),
        infra.valkey_pool.clone(),
        sync_trigger.clone(),
        sync_lock.clone(),
        veronex::domain::constants::SYNC_LOOP_BASE_TICK,
        shutdown.child_token(),
        infra.http_client.clone(),
        repos.ollama_model_repo.clone(),
        repos.model_selection_repo.clone(),
        repos.vram_budget_repo.clone(),
    ));
    tracing::info!("sync loop started (analyzer: {})", config.analyzer_url);

    // ── Session grouping loop ──────────────────────────────────────
    let session_grouping_lock = Arc::new(tokio::sync::Semaphore::new(1));
    tasks.spawn(run_session_grouping_loop(
        Arc::new(infra.pg_pool.clone()),
        session_grouping_lock.clone(),
        Duration::from_secs(config.session_grouping_interval_secs),
        shutdown.child_token(),
    ));
    tracing::info!(
        "session grouping loop started (interval={}s)",
        config.session_grouping_interval_secs
    );

    // ── Job event broadcast channel ────────────────────────────────
    let (job_event_tx, _) = tokio::sync::broadcast::channel::<JobStatusEvent>(256);
    let job_event_tx = Arc::new(job_event_tx);

    // ── Provider dispatch + inference use case ─────────────────────
    let provider_dispatch = Arc::new(ConcreteProviderDispatch::new(
        repos.provider_registry.clone(),
        Some(repos.gemini_policy_repo.clone()),
        Some(repos.model_selection_repo.clone()),
        Some(repos.ollama_model_repo.clone()),
        infra.valkey_pool.clone(),
    ));

    let use_case_impl = Arc::new(InferenceUseCaseImpl::new(
        repos.provider_registry.clone(),
        repos.job_repo.clone(),
        repos.valkey_port.clone(),
        repos.observability.clone(),
        repos.model_manager.clone(),
        repos.vram_pool.clone(),
        thermal.clone(),
        circuit_breaker.clone(),
        provider_dispatch,
        (*job_event_tx).clone(),
        repos.message_store.clone(),
        Some(repos.ollama_model_repo.clone()),
        Some(repos.model_selection_repo.clone()),
        infra.instance_id.clone(),
    ));

    if let Err(e) = use_case_impl.recover_pending_jobs().await {
        tracing::warn!("job recovery failed (non-fatal): {e}");
    }
    tasks.spawn(use_case_impl.start_queue_worker(shutdown.child_token()));
    tasks.spawn(use_case_impl.start_job_sweeper(shutdown.child_token()));

    // ── Multi-instance pub/sub + reaper (when Valkey is available) ──
    if let Some(pool) = &infra.valkey_pool {
        use fred::clients::SubscriberClient;
        use fred::interfaces::ClientLike;
        use veronex::infrastructure::outbound::pubsub::{reaper, relay};

        // Job event subscriber
        let valkey_url = config.valkey_url.as_deref().expect("VALKEY_URL required when Valkey pool is present");
        let event_sub_config = fred::types::config::Config::from_url(valkey_url)
        .expect("invalid VALKEY_URL for event subscriber");
        let event_subscriber = SubscriberClient::new(event_sub_config, None, None, None);
        if let Err(e) = event_subscriber.init().await {
            tracing::error!("job event subscriber init failed — event relay disabled: {e}");
        }
        let event_tx_clone = (*job_event_tx).clone();
        let iid = infra.instance_id.clone();
        tasks.spawn(relay::run_job_event_subscriber(
            event_subscriber,
            event_tx_clone,
            iid,
            shutdown.child_token(),
        ));

        // Cancel subscriber
        let cancel_sub_config = fred::types::config::Config::from_url(valkey_url)
        .expect("invalid VALKEY_URL for cancel subscriber");
        let cancel_subscriber = SubscriberClient::new(cancel_sub_config, None, None, None);
        if let Err(e) = cancel_subscriber.init().await {
            tracing::error!("cancel subscriber init failed — cross-node cancel disabled: {e}");
        }
        let cancel_notifiers = use_case_impl.cancel_notifiers();
        tasks.spawn(relay::run_cancel_subscriber(
            cancel_subscriber,
            cancel_notifiers,
            shutdown.child_token(),
        ));

        // Reaper: heartbeat + VRAM lease reaping + orphaned job re-enqueue
        let distributed_vram_pool = Some(Arc::new(
            veronex::infrastructure::outbound::capacity::distributed_vram_pool::DistributedVramPool::new(
                pool.clone(),
                infra.instance_id.clone(),
            ),
        ));
        tasks.spawn(reaper::run_reaper_loop(
            pool.clone(),
            infra.instance_id.clone(),
            distributed_vram_pool,
            shutdown.child_token(),
        ));

        tracing::info!("multi-instance coordination enabled (pub/sub + reaper)");

        // ── Queue maintenance: promote_overdue + demand_resync (Phase 4) ──
        use veronex::infrastructure::outbound::queue_maintenance;

        tasks.spawn(queue_maintenance::run_promote_overdue_loop(
            pool.clone(),
            Duration::from_secs(veronex::domain::constants::OVERDUE_PROMOTE_SECS),
            shutdown.child_token(),
        ));

        tasks.spawn(queue_maintenance::run_demand_resync_loop(
            pool.clone(),
            Duration::from_secs(veronex::domain::constants::DEMAND_RESYNC_SECS),
            shutdown.child_token(),
        ));

        if let Some(ref vk_port) = repos.valkey_port {
            tasks.spawn(queue_maintenance::run_queue_wait_cancel_loop(
                pool.clone(),
                vk_port.clone(),
                repos.job_repo.clone(),
                Duration::from_secs(veronex::domain::constants::QUEUE_WAIT_CANCEL_SECS),
                shutdown.child_token(),
            ));
        }

        tracing::info!("queue maintenance loops started (promote_overdue=30s, demand_resync=60s, queue_wait_cancel=30s)");

        // ── Placement Planner (Phase 5) ──────────────────────────────────
        if let Some(ref vk_port) = repos.valkey_port {
            tasks.spawn(veronex::application::use_cases::placement_planner::run_placement_planner_loop(
                repos.provider_registry.clone(),
                repos.vram_pool.clone(),
                thermal.clone(),
                circuit_breaker.clone(),
                vk_port.clone(),
                infra.http_client.clone(),
                infra.instance_id.clone(),
                use_case_impl.as_thermal_drain(),
                shutdown.child_token(),
            ));
            tracing::info!("placement planner started (interval=5s)");
        }
    }

    let use_case: Arc<dyn InferenceUseCase> = use_case_impl;

    BackgroundHandles {
        thermal,
        circuit_breaker,
        sync_trigger,
        sync_lock,
        session_grouping_lock,
        job_event_tx,
        use_case,
    }
}
