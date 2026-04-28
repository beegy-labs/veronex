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
use veronex::infrastructure::outbound::health_checker::{run_health_checker_loop, run_server_metrics_loop};
use veronex::infrastructure::outbound::provider_dispatch::ConcreteProviderDispatch;
use veronex::infrastructure::outbound::session_grouping::run_session_grouping_loop;
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
    /// Replayed to new SSE clients on connect so late joiners see recent activity.
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
    let thermal = Arc::new(ThermalThrottleMap::new(
        veronex::domain::constants::THERMAL_HARD_COOLDOWN_SECS as u64,
    ));
    let circuit_breaker = Arc::new(CircuitBreakerMap::new());
    let sync_trigger = Arc::new(tokio::sync::Notify::new());
    let sync_lock = Arc::new(tokio::sync::Semaphore::new(1));

    // ── Health checker ─────────────────────────────────────────────
    tasks.spawn(run_health_checker_loop(
        repos.provider_registry.clone(),
        repos.gpu_server_registry.clone(),
        veronex::domain::constants::HEALTH_CHECK_INTERVAL_SECS,
        infra.valkey_pool.clone(),
        thermal.clone(),
        shutdown.child_token(),
        infra.http_client.clone(),
        repos.vram_pool.clone(),
        infra.pg_pool.clone(),
        infra.instance_id.clone(),
        config.analytics_url.clone(),
        std::env::var("S3_ENDPOINT").ok(),
        std::env::var("VESPA_URL").ok(),
        std::env::var("EMBED_URL").ok(),
    ));

    // ── Server metrics scrape loop (independent of providers) ──────
    tasks.spawn(run_server_metrics_loop(
        repos.gpu_server_registry.clone(),
        infra.valkey_pool.clone(),
        infra.pg_pool.clone(),
        veronex::domain::constants::HEALTH_CHECK_INTERVAL_SECS,
        shutdown.child_token(),
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

    // ── MCP lifecycle phase feature flag ────────────────────────────
    // SDD: `.specs/veronex/inference-lifecycle-sod.md` §7.1b. Default off
    // — flipped to `on` after live verification on dev cluster.
    let mcp_lifecycle_phase_enabled = std::env::var(
        veronex::domain::constants::MCP_LIFECYCLE_PHASE_FLAG_ENV,
    )
    .ok()
    .and_then(|v| match v.to_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    })
    .unwrap_or(veronex::domain::constants::MCP_LIFECYCLE_PHASE_DEFAULT);
    tracing::info!(
        enabled = mcp_lifecycle_phase_enabled,
        env = veronex::domain::constants::MCP_LIFECYCLE_PHASE_FLAG_ENV,
        "mcp lifecycle phase"
    );

    // ── Provider dispatch + inference use case ─────────────────────
    let provider_dispatch = Arc::new(ConcreteProviderDispatch::new(
        repos.provider_registry.clone(),
        Some(repos.gemini_policy_repo.clone()),
        Some(repos.model_selection_repo.clone()),
        Some(repos.ollama_model_repo.clone()),
        infra.valkey_pool.clone(),
        Some(repos.vram_pool.clone()),
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
        Some(repos.global_model_settings_repo.clone()),
        infra.instance_id.clone(),
        Some(repos.lab_settings_repo.clone()),
        mcp_lifecycle_phase_enabled,
    ));

    if let Err(e) = use_case_impl.recover_pending_jobs().await {
        tracing::warn!("job recovery failed (non-fatal): {e}");
    }

    // ── Startup: register instance in global set for orphan sweeper ──
    if let Some(ref vk) = infra.valkey_pool {
        use fred::interfaces::SetsInterface;
        let _: Result<i64, _> = vk
            .sadd(
                veronex::infrastructure::outbound::valkey_keys::instances_set(),
                infra.instance_id.as_ref(),
            )
            .await;
    }

    // ── Startup reconciliation: seed Valkey job counters from DB ──
    if let Some(ref vk) = infra.valkey_pool {
        use fred::interfaces::KeysInterface;
        use veronex::infrastructure::outbound::valkey_keys as vkeys;
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status::text, COUNT(*) FROM inference_jobs \
             WHERE status IN ('pending','running') GROUP BY status"
        )
        .fetch_all(&infra.pg_pool)
        .await
        .unwrap_or_default();
        let mut pending = 0i64;
        let mut running = 0i64;
        for (status, cnt) in &rows {
            match status.as_str() {
                "pending" => pending = *cnt,
                "running" => running = *cnt,
                _ => {}
            }
        }
        vk.set(vkeys::jobs_pending_counter(), pending, None, None, false).await
            .unwrap_or_else(|e| tracing::warn!(error = %e, "Valkey SET jobs_pending_counter failed"));
        vk.set(vkeys::jobs_running_counter(), running, None, None, false).await
            .unwrap_or_else(|e| tracing::warn!(error = %e, "Valkey SET jobs_running_counter failed"));
        tracing::info!(pending, running, "job counters seeded from DB");
    }

    // NOTE: Startup stuck-job cleanup moved to veronex-agent orphan sweeper.
    // The agent monitors heartbeats and cleans up orphaned jobs with a 2-minute
    // grace period, preventing false positives from network blips.

    tasks.spawn(use_case_impl.start_queue_worker(shutdown.child_token()));
    tasks.spawn(use_case_impl.start_job_sweeper(shutdown.child_token()));

    // ── Real-time stats ticker ──────────────────────────────────────
    // Computes FlowStats every second from sliding-window counters + live DashMap counts
    // and broadcasts to all SSE clients so every user sees the same numbers.
    // Capacity 16: ticker fires at most once per second; 16 slots = 16s of lag tolerance.
    // Always broadcast — no PartialEq skip — clients rely on receiving stats every second.
    let (stats_tx, _) = tokio::sync::broadcast::channel::<FlowStats>(16);
    let stats_tx = Arc::new(stats_tx);
    {
        let pg        = infra.pg_pool.clone();
        let vk_pool   = infra.valkey_pool.clone();
        let tx        = (*stats_tx).clone();
        let shutdown_token = shutdown.clone();

        // Sliding-window counters — 60 × 1-second buckets.
        // req/s = sum of last 10 buckets / 10.  req/m = sum of all 60 buckets.
        // Unlike the ring buffer (which evicts oldest events), these counters only
        // accumulate and rotate, so they accurately reflect the rate.
        let incoming_buckets = Arc::new(std::sync::Mutex::new([0u32; 60]));
        let completed_buckets = Arc::new(std::sync::Mutex::new([0u32; 60]));
        let bucket_idx = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Separate task: count incoming/completed from broadcast events into buckets
        {
            let mut rx = job_event_tx.subscribe();
            let inc_b = incoming_buckets.clone();
            let done_b = completed_buckets.clone();
            let bi = bucket_idx.clone();
            let shutdown_token = shutdown.clone();
            tasks.spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown_token.cancelled() => break,
                        result = rx.recv() => match result {
                            Ok(event) => {
                                let idx = bi.load(std::sync::atomic::Ordering::Relaxed) % 60;
                                match event.status.as_str() {
                                    "pending" => {
                                        let mut b = inc_b.lock().unwrap();
                                        b[idx] += 1;
                                    }
                                    "completed" | "failed" | "cancelled" => {
                                        let mut b = done_b.lock().unwrap();
                                        b[idx] += 1;
                                    }
                                    _ => {}
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            });
        }
        tasks.spawn(async move {
            let mut interval = tokio::time::interval(STATS_TICK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut tick_count = 0u64;
            // Cached pending/running counts for no-Valkey fallback (re-queried every 10s)
            let mut cached_no_valkey: (u32, u32) = (0, 0);
            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => break,
                    _ = interval.tick() => {
                        // Rotate bucket every tick: advance index, clear the new bucket
                        tick_count += 1;
                        let new_idx = (tick_count as usize) % 60;
                        bucket_idx.store(new_idx, std::sync::atomic::Ordering::Relaxed);
                        {
                            let mut b = incoming_buckets.lock().unwrap();
                            b[new_idx] = 0;
                        }
                        {
                            let mut b = completed_buckets.lock().unwrap();
                            b[new_idx] = 0;
                        }

                        // req/s: sum last 10 buckets.  req/m: sum all 60 buckets.
                        let (incoming, incoming_60s) = {
                            let b = incoming_buckets.lock().unwrap();
                            let mut sum_10 = 0u32;
                            let mut sum_60 = 0u32;
                            for i in 0..60 {
                                let val = b[i];
                                sum_60 += val;
                                // Last 10 buckets = indices (new_idx-9)..=new_idx (wrapping)
                                let age = (new_idx + 60 - i) % 60;
                                if age > 0 && age <= 10 { sum_10 += val; }
                            }
                            (sum_10, sum_60)
                        };
                        let completed = {
                            let b = completed_buckets.lock().unwrap();
                            b.iter().sum::<u32>()
                        };

                        // Pending/running counts: read from Valkey atomic counters (O(1)).
                        // Reconciled from DB every 60 ticks to correct any drift.
                        let (queued, running) = if let Some(ref vk) = vk_pool {
                            use fred::interfaces::KeysInterface;
                            use veronex::infrastructure::outbound::valkey_keys as vkeys2;

                            let p: i64 = vk.get(vkeys2::jobs_pending_counter()).await.unwrap_or(0);
                            let r: i64 = vk.get(vkeys2::jobs_running_counter()).await.unwrap_or(0);

                            // Periodic reconciliation: every 60 ticks, verify against DB
                            if tick_count % 60 == 0 {
                                let rows: Vec<(String, i64)> = sqlx::query_as(
                                    "SELECT status::text, COUNT(*) FROM inference_jobs \
                                     WHERE status IN ('pending','running') GROUP BY status"
                                )
                                .fetch_all(&pg)
                                .await
                                .unwrap_or_default();
                                let mut db_p = 0i64;
                                let mut db_r = 0i64;
                                for (status, cnt) in &rows {
                                    match status.as_str() {
                                        "pending" => db_p = *cnt,
                                        "running" => db_r = *cnt,
                                        _ => {}
                                    }
                                }
                                if db_p != p {
                                    tracing::debug!(valkey = p, db = db_p, "reconciling pending counter");
                                    vk.set(vkeys2::jobs_pending_counter(), db_p, None, None, false).await
                                        .unwrap_or_else(|e| tracing::warn!(error = %e, "Valkey SET jobs_pending reconcile failed"));
                                }
                                if db_r != r {
                                    tracing::debug!(valkey = r, db = db_r, "reconciling running counter");
                                    vk.set(vkeys2::jobs_running_counter(), db_r, None, None, false).await
                                        .unwrap_or_else(|e| tracing::warn!(error = %e, "Valkey SET jobs_running reconcile failed"));
                                }
                                (db_p.max(0) as u32, db_r.max(0) as u32)
                            } else {
                                (p.max(0) as u32, r.max(0) as u32)
                            }
                        } else {
                            // No Valkey — fall back to DB query, cached for 10 ticks (10s)
                            if tick_count % 10 == 1 {
                                let rows: Vec<(String, i64)> = sqlx::query_as(
                                    "SELECT status::text, COUNT(*) FROM inference_jobs \
                                     WHERE status IN ('pending','running') GROUP BY status"
                                )
                                .fetch_all(&pg)
                                .await
                                .unwrap_or_default();
                                let mut p = 0u32;
                                let mut r = 0u32;
                                for (status, cnt) in &rows {
                                    match status.as_str() {
                                        "pending" => p = *cnt as u32,
                                        "running" => r = *cnt as u32,
                                        _ => {}
                                    }
                                }
                                cached_no_valkey = (p, r);
                            }
                            cached_no_valkey
                        };

                        let stats = FlowStats { incoming, incoming_60s, queued, running, completed };
                        // Always broadcast — clients rely on receiving stats every second.
                        // The 16-slot channel absorbs bursts without backpressure.
                        let _ = tx.send(stats);
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
            infra.pg_pool.clone(),
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

        if let Some(ref vk_port) = repos.valkey_port {
            tasks.spawn(queue_maintenance::run_processing_reaper_loop(
                vk_port.clone(),
                repos.job_repo.clone(),
                Duration::from_secs(veronex::domain::constants::PROCESSING_REAPER_SECS),
                shutdown.child_token(),
            ));
        }

        tracing::info!("queue maintenance loops started (promote_overdue=30s, demand_resync=60s, queue_wait_cancel=30s, processing_reaper=30s)");

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
                Some(repos.ollama_model_repo.clone()),
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
#[cfg(test)]
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
