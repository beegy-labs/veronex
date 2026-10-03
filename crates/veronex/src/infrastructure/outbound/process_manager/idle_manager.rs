//! Phase 3 — idle reaper. Stops managed processes whose
//! [`ActivityTracker::idle_for`] exceeds the configured TTL.
//!
//! TTL resolution priority:
//! 1. Per-provider override (`llm_providers.idle_ttl_seconds_override`)
//!    — passed in via the `provider_overrides` callback so we don't
//!    couple this module to the registry implementation.
//! 2. Global setting `llama_server.idle_ttl_seconds` from
//!    [`SystemSettingsRepository`].
//! 3. [`DEFAULT_TTL_SECONDS`] (60s) when both are absent.
//!
//! `0` means "never reap" — explicit opt-out for development setups
//! where the operator wants the process to stay warm regardless of
//! activity.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::application::ports::outbound::system_settings_repository::{
    self as ss, SystemSettingsRepository,
};

use super::activity_tracker::ActivityTracker;
use super::manager::{ProcessManager, RunningProcess};

pub const DEFAULT_TTL_SECONDS: u64 = 60;

/// Closure type that returns the per-provider override if any. The
/// loop calls this for every Ready process; production hooks it into
/// `LlmProviderRegistry::get`, tests pass a fixed map.
pub type ProviderOverrideFn = Arc<dyn Fn(Uuid) -> Option<i32> + Send + Sync>;

#[derive(Clone)]
pub struct IdleManager {
    activity: ActivityTracker,
    manager: ProcessManager,
    settings: Arc<dyn SystemSettingsRepository>,
    override_fn: ProviderOverrideFn,
}

impl IdleManager {
    pub fn new(
        activity: ActivityTracker,
        manager: ProcessManager,
        settings: Arc<dyn SystemSettingsRepository>,
        override_fn: ProviderOverrideFn,
    ) -> Self {
        Self {
            activity,
            manager,
            settings,
            override_fn,
        }
    }

    /// Run one tick. Iterates running providers, evaluates idle, stops
    /// the ones over TTL. Errors are logged — never propagated, so a
    /// single failure doesn't poison the loop.
    pub async fn tick(&self) {
        let global_ttl = self.global_ttl_seconds().await;
        let now = Utc::now();
        let providers: Vec<RunningProcess> = self.manager.list_running();
        for p in providers {
            let override_secs = (self.override_fn)(p.provider_id);
            let ttl_secs = override_secs
                .map(|v| v.max(0) as u64)
                .unwrap_or(global_ttl);
            if ttl_secs == 0 {
                continue;
            }
            let Some(idle) = self.activity.idle_for(p.provider_id, now) else {
                continue;
            };
            if idle.as_secs() >= ttl_secs {
                tracing::info!(
                    provider_id = %p.provider_id,
                    model_id = %p.model_id,
                    idle_secs = idle.as_secs(),
                    ttl_secs,
                    "idle TTL elapsed — stopping managed process",
                );
                if let Err(e) = self.manager.stop(p.provider_id).await {
                    tracing::warn!(
                        provider_id = %p.provider_id,
                        error = %e,
                        "idle stop failed",
                    );
                }
            }
        }
    }

    async fn global_ttl_seconds(&self) -> u64 {
        match self.settings.get(ss::KEY_IDLE_TTL_SECONDS).await {
            Ok(Some(s)) => s.value.parse::<u64>().unwrap_or(DEFAULT_TTL_SECONDS),
            Ok(None) => ss::default_for(ss::KEY_IDLE_TTL_SECONDS)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(DEFAULT_TTL_SECONDS),
            Err(e) => {
                tracing::warn!(error = %e, "failed to read idle_ttl_seconds — using default");
                DEFAULT_TTL_SECONDS
            }
        }
    }

    /// Background loop entry. Tick every `interval`; bail when the
    /// supplied cancellation token is triggered.
    pub async fn run(self, interval: Duration, cancel: tokio_util::sync::CancellationToken) {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = ticker.tick() => self.tick().await,
                _ = cancel.cancelled() => break,
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::application::ports::outbound::system_settings_repository::SystemSetting;
    use crate::infrastructure::outbound::capacity::aimd_registry::AimdRegistry;
    use crate::infrastructure::outbound::process_manager::agent_client::StubNodeClient;
    use crate::infrastructure::outbound::process_manager::port_pool::PortPool;
    use anyhow::Result;
    use async_trait::async_trait;

    /// In-memory settings repo with a configurable global TTL.
    struct FakeSettings(Option<String>);

    #[async_trait]
    impl SystemSettingsRepository for FakeSettings {
        async fn get(&self, key: &str) -> Result<Option<SystemSetting>> {
            if key == ss::KEY_IDLE_TTL_SECONDS
                && let Some(v) = &self.0
            {
                return Ok(Some(SystemSetting {
                    key: key.to_string(),
                    value: v.clone(),
                    description: None,
                    updated_at: Utc::now(),
                    updated_by: None,
                }));
            }
            Ok(None)
        }
        async fn list_all(&self) -> Result<Vec<SystemSetting>> {
            Ok(vec![])
        }
        async fn upsert(
            &self,
            _key: &str,
            _value: &str,
            _desc: Option<&str>,
            _by: Option<Uuid>,
        ) -> Result<()> {
            Ok(())
        }
        async fn delete(&self, _key: &str) -> Result<bool> {
            Ok(false)
        }
    }

    async fn ready_provider(
        mgr: &ProcessManager,
        node_id: Uuid,
    ) -> (Uuid, RunningProcess) {
        let pid = Uuid::now_v7();
        let rp = mgr
            .ensure_running(
                pid,
                node_id,
                "qwen3:q4".into(),
                "sha".into(),
                serde_json::json!({}),
                4,
            )
            .await
            .unwrap();
        (pid, rp)
    }

    #[tokio::test]
    async fn tick_stops_provider_past_ttl() {
        let activity = ActivityTracker::new();
        let aimd = AimdRegistry::new();
        let mgr = ProcessManager::new(activity.clone(), aimd);
        let node = Uuid::now_v7();
        mgr.register_node(node, Arc::new(StubNodeClient::default()), PortPool::new(20000..=20002));
        let (pid, _) = ready_provider(&mgr, node).await;

        // Backdate the activity so idle_for exceeds the TTL.
        activity.unregister(pid);
        activity.register(pid, Utc::now() - chrono::Duration::seconds(120));

        let idle = IdleManager::new(
            activity.clone(),
            mgr.clone(),
            Arc::new(FakeSettings(Some("60".to_string()))),
            Arc::new(|_| None),
        );
        idle.tick().await;
        assert!(mgr.get(pid).is_none());
    }

    #[tokio::test]
    async fn ttl_zero_disables_reaping() {
        let activity = ActivityTracker::new();
        let mgr = ProcessManager::new(activity.clone(), AimdRegistry::new());
        let node = Uuid::now_v7();
        mgr.register_node(node, Arc::new(StubNodeClient::default()), PortPool::new(20000..=20002));
        let (pid, _) = ready_provider(&mgr, node).await;

        // Force long idle.
        activity.unregister(pid);
        activity.register(pid, Utc::now() - chrono::Duration::seconds(3600));

        let idle = IdleManager::new(
            activity,
            mgr.clone(),
            Arc::new(FakeSettings(Some("0".to_string()))),
            Arc::new(|_| None),
        );
        idle.tick().await;
        assert!(mgr.get(pid).is_some(), "ttl=0 must skip reaping");
    }

    #[tokio::test]
    async fn override_beats_global_setting() {
        let activity = ActivityTracker::new();
        let mgr = ProcessManager::new(activity.clone(), AimdRegistry::new());
        let node = Uuid::now_v7();
        mgr.register_node(node, Arc::new(StubNodeClient::default()), PortPool::new(20000..=20002));
        let (pid, _) = ready_provider(&mgr, node).await;
        activity.unregister(pid);
        activity.register(pid, Utc::now() - chrono::Duration::seconds(40));

        // Global says 60 (would NOT reap at idle=40), override says 30
        // (WILL reap). Override must win.
        let pid_for_closure = pid;
        let idle = IdleManager::new(
            activity,
            mgr.clone(),
            Arc::new(FakeSettings(Some("60".to_string()))),
            Arc::new(move |q| if q == pid_for_closure { Some(30) } else { None }),
        );
        idle.tick().await;
        assert!(mgr.get(pid).is_none());
    }
}
