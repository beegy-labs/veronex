use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use veronex::application::ports::inbound::inference_use_case::InferenceUseCase;
use veronex::application::use_cases::InferenceUseCaseImpl;
use veronex::domain::value_objects::{FlowStats, JobStatusEvent};
use veronex::infrastructure::outbound::capacity::analyzer::run_sync_loop;
use veronex::infrastructure::outbound::capacity::thermal::ThermalThrottleMap;
use veronex::infrastructure::outbound::circuit_breaker::CircuitBreakerMap;
use veronex::infrastructure::outbound::health_checker::run_health_checker_loop;
use veronex::infrastructure::outbound::provider_dispatch::ConcreteProviderDispatch;
use veronex::infrastructure::outbound::session_grouping::run_session_grouping_loop;
use veronex::infrastructure::outbound::valkey_keys::{QUEUE_JOBS, QUEUE_JOBS_PAID, QUEUE_JOBS_TEST};
use veronex::domain::constants::STATS_TICK_INTERVAL;

use super::config::AppConfig;
use super::repositories::Repositories;

/// Infrastructure context shared between repository wiring and background tasks.
pub struct InfraContext {
    pub valkey_pool: Option<fred::clients::Pool>,
    pub pg_pool: sqlx::PgPool,
    pub http_client: reqwest::Client,
    pub instance_id: Arc<str>,
}

/// Maximum number of recent job events retained in the replay buffer.
/// New SSE clients receive these events on connect so all users see the same view.
const EVENT_BUFFER_CAPACITY: usize = 100;

/// All shared infrastructure handles created during background task setup.
/// Returned so `main` can wire them into `AppState`.
pub struct BackgroundHandles {
    pub thermal: Arc<ThermalThrottleMap>,
    pub circuit_breaker: Arc<CircuitBreakerMap>,
    pub sync_trigger: Arc<tokio::sync::Notify>,
    pub sync_lock: Arc<tokio::sync::Semaphore>,
    pub session_grouping_lock: Arc<tokio::sync::Semaphore>,
    pub job_event_tx: Arc<tokio::sync::broadcast::Sender<JobStatusEvent>>,
    /// Rolling replay buffer — `(event, unix_ms)` tuples, last `EVENT_BUFFER_CAPACITY` entries.
    /// Replayed to new SSE clients on connect; unix_ms used by the stats ticker.
    pub event_ring_buffer: Arc<RwLock<VecDeque<(JobStatusEvent, u64)>>>,
    /// Broadcast channel for real-time aggregate stats pushed every second.
    /// All SSE clients receive the same FlowStats simultaneously.
    pub stats_tx: Arc<tokio::sync::broadcast::Sender<FlowStats>>,
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
        Some(repos.job_repo.clone() as Arc<dyn veronex::application::ports::outbound::job_repository::JobRepository>),
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

    // ── Job event broadcast channel + replay ring buffer ───────────
    let (job_event_tx, _) = tokio::sync::broadcast::channel::<JobStatusEvent>(256);
    let job_event_tx = Arc::new(job_event_tx);

    // Ring buffer: subscribe to broadcast channel, keep last EVENT_BUFFER_CAPACITY
    // events with server-side unix_ms timestamps for replay and stats computation.
    let event_ring_buffer: Arc<RwLock<VecDeque<(JobStatusEvent, u64)>>> =
        Arc::new(RwLock::new(VecDeque::with_capacity(EVENT_BUFFER_CAPACITY)));
    {
        let mut rx = job_event_tx.subscribe();
        let buf = event_ring_buffer.clone();
        let shutdown_token = shutdown.clone();
        tasks.spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => break,
                    result = rx.recv() => match result {
                        Ok(event) => {
                            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                            let mut buf = buf.write().expect("ring buffer poisoned");
                            if buf.len() >= EVENT_BUFFER_CAPACITY {
                                buf.pop_front();
                            }
                            buf.push_back((event, now_ms));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        });
    }

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
        repos.image_store.clone(),
        Some(repos.ollama_model_repo.clone()),
        Some(repos.model_selection_repo.clone()),
        infra.instance_id.clone(),
    ));

    if let Err(e) = use_case_impl.recover_pending_jobs().await {
        tracing::warn!("job recovery failed (non-fatal): {e}");
    }
    tasks.spawn(use_case_impl.start_queue_worker(shutdown.child_token()));
    tasks.spawn(use_case_impl.start_job_sweeper(shutdown.child_token()));

    // ── Real-time stats ticker ──────────────────────────────────────
    // Computes FlowStats every second from the ring buffer + live DashMap counts
    // and broadcasts to all SSE clients so every user sees the same numbers.
    // Capacity 16: ticker fires at most once per second; 16 slots = 16s of lag tolerance.
    // No-op guard (PartialEq) ensures capacity is only consumed when stats actually change.
    let (stats_tx, _) = tokio::sync::broadcast::channel::<FlowStats>(16);
    let stats_tx = Arc::new(stats_tx);
    {
        use fred::prelude::*;

        let buf       = event_ring_buffer.clone();
        let uc        = use_case_impl.clone() as Arc<dyn InferenceUseCase>;
        let valkey    = infra.valkey_pool.clone();
        let tx        = (*stats_tx).clone();
        let shutdown_token = shutdown.clone();

        tasks.spawn(async move {
            let mut interval = tokio::time::interval(STATS_TICK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_stats: Option<FlowStats> = None;
            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                        let window = now_ms.saturating_sub(1_000);

                        // Per-second rates from ring buffer — single pass O(100)
                        let (incoming, completed) = {
                            let buf = buf.read().expect("ring buffer poisoned");
                            count_ring_events(&buf, window)
                        };

                        // Instantaneous counts from in-memory DashMap
                        let live = uc.get_live_counts();

                        // Queue depth from Valkey (0 if unavailable)
                        let queued = if let Some(ref pool) = valkey {
                            let (paid, api, test): (i64, i64, i64) = tokio::join!(
                                async { pool.llen::<i64, _>(QUEUE_JOBS_PAID).await.unwrap_or(0) },
                                async { pool.llen::<i64, _>(QUEUE_JOBS).await.unwrap_or(0) },
                                async { pool.llen::<i64, _>(QUEUE_JOBS_TEST).await.unwrap_or(0) },
                            );
                            (paid + api + test).max(0) as u32
                        } else {
                            live.pending
                        };

                        let stats = FlowStats { incoming, queued, running: live.running, completed };
                        // Always send the first tick; skip subsequent broadcasts when nothing changed.
                        if last_stats.as_ref() != Some(&stats) {
                            last_stats = Some(stats.clone());
                            let _ = tx.send(stats);
                        }
                    }
                }
            }
        });
    }

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
        event_ring_buffer,
        stats_tx,
        use_case,
    }
}

/// Count `(incoming, completed)` events in the ring buffer that fall within `[window_ms, ∞)`.
///
/// - `incoming` = `"pending"` transitions (job arrivals, ~req/s)
/// - `completed` = `"completed" | "failed" | "cancelled"` (terminal outcomes)
///
/// Extracted from the stats ticker closure to make the counting logic unit-testable.
pub(super) fn count_ring_events(
    buf: &std::collections::VecDeque<(JobStatusEvent, u64)>,
    window_ms: u64,
) -> (u32, u32) {
    let mut inc = 0u32;
    let mut done = 0u32;
    for (e, ts) in buf.iter() {
        if *ts < window_ms { continue; }
        match e.status.as_str() {
            "pending" => inc += 1,
            "completed" | "failed" | "cancelled" => done += 1,
            _ => {}
        }
    }
    (inc, done)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use veronex::domain::value_objects::JobStatusEvent;

    fn make_event(status: &str, ts: u64) -> (JobStatusEvent, u64) {
        let ev = JobStatusEvent {
            id: "test".to_string(),
            status: status.to_string(),
            model_name: "test-model".to_string(),
            provider_type: "ollama".to_string(),
            latency_ms: None,
        };
        (ev, ts)
    }

    #[test]
    fn empty_buffer_returns_zeros() {
        let buf = std::collections::VecDeque::new();
        assert_eq!(count_ring_events(&buf, 1000), (0, 0));
    }

    #[test]
    fn events_outside_window_ignored() {
        let mut buf = std::collections::VecDeque::new();
        buf.push_back(make_event("pending", 500));
        buf.push_back(make_event("completed", 999));
        // window_ms = 1000 → ts 500 and 999 are both excluded
        assert_eq!(count_ring_events(&buf, 1000), (0, 0));
    }

    #[test]
    fn boundary_ts_equal_to_window_is_included() {
        let mut buf = std::collections::VecDeque::new();
        buf.push_back(make_event("pending", 1000));
        assert_eq!(count_ring_events(&buf, 1000), (1, 0));
    }

    #[test]
    fn counts_mixed_statuses_within_window() {
        let mut buf = std::collections::VecDeque::new();
        buf.push_back(make_event("pending",   1000));
        buf.push_back(make_event("running",   1001)); // not counted
        buf.push_back(make_event("completed", 1002));
        buf.push_back(make_event("failed",    1003));
        buf.push_back(make_event("cancelled", 1004));
        buf.push_back(make_event("pending",   1005));
        let (inc, done) = count_ring_events(&buf, 1000);
        assert_eq!(inc, 2);
        assert_eq!(done, 3);
    }
}
