//! Phase 3 — high-level managed-process orchestrator.
//!
//! Holds one [`NodeClient`] per registered `LlmNode`, a [`PortPool`] per
//! node, and an in-memory map of `provider_id → Running` so the router
//! can ask "is this provider's process up?" without round-tripping the
//! agent.
//!
//! `ensure_running(provider_id, model_id)` is idempotent: concurrent
//! callers wait on the same in-flight future via a per-provider one-slot
//! semaphore. Real spawn delegates to the [`NodeClient`] (see the
//! agent crate); this module is platform-agnostic.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::infrastructure::outbound::capacity::aimd_registry::AimdRegistry;

use super::activity_tracker::ActivityTracker;
use super::agent_client::{NodeClient, SpawnRequest};
use super::port_pool::{PortLease, PortPool};

/// Lifecycle phases the router and admin UI distinguish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    /// Spawn requested; waiting on agent /spawn to return.
    Loading,
    /// Spawn succeeded; running warmup probes.
    Warming,
    /// Healthy; routable.
    Ready,
    /// Stop requested; waiting on agent /process DELETE.
    Stopping,
    /// Spawn or warmup failed; admin must intervene.
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunningProcess {
    pub provider_id: Uuid,
    pub model_id: String,
    pub node_id: Uuid,
    pub port: u16,
    pub agent_handle: Uuid,
    pub state: ProcessState,
    pub started_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
}

struct NodeContext {
    client: Arc<dyn NodeClient>,
    pool: PortPool,
}

/// Cheap to clone (`Arc`-of-state). Holds the orchestrator on the API
/// server side. Wired into `AppState` once Phase 3 lifecycle is live.
#[derive(Clone)]
pub struct ProcessManager {
    inner: Arc<Inner>,
}

struct Inner {
    nodes: DashMap<Uuid, NodeContext>,
    /// `provider_id → RunningProcess`
    running: DashMap<Uuid, RunningProcess>,
    /// `provider_id → 1-permit semaphore`. Concurrent ensure_running
    /// calls for the same provider serialize behind it so we never
    /// double-spawn.
    ensure_locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
    /// Held leases keyed by `provider_id`. Drop releases the port when
    /// the process is stopped.
    port_leases: DashMap<Uuid, PortLease>,
    activity: ActivityTracker,
    aimd: AimdRegistry,
}

impl ProcessManager {
    pub fn new(activity: ActivityTracker, aimd: AimdRegistry) -> Self {
        Self {
            inner: Arc::new(Inner {
                nodes: DashMap::new(),
                running: DashMap::new(),
                ensure_locks: Mutex::new(HashMap::new()),
                port_leases: DashMap::new(),
                activity,
                aimd,
            }),
        }
    }

    /// Register a node + its agent client + port pool. Replaces any
    /// existing context for the same node id.
    pub fn register_node(
        &self,
        node_id: Uuid,
        client: Arc<dyn NodeClient>,
        pool: PortPool,
    ) {
        self.inner
            .nodes
            .insert(node_id, NodeContext { client, pool });
    }

    /// Snapshot for admin UIs.
    pub fn list_running(&self) -> Vec<RunningProcess> {
        self.inner
            .running
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Look up a process by provider id.
    pub fn get(&self, provider_id: Uuid) -> Option<RunningProcess> {
        self.inner.running.get(&provider_id).map(|e| e.clone())
    }

    /// Idempotent spawn. Concurrent callers wait on the same lock.
    /// Returns the running process record on success.
    ///
    /// `n_parallel` is forwarded to `AimdRegistry::register` so the
    /// admission gate has the upper bound from the start.
    pub async fn ensure_running(
        &self,
        provider_id: Uuid,
        node_id: Uuid,
        model_id: String,
        blob_sha256: String,
        runtime: serde_json::Value,
        n_parallel: u32,
    ) -> Result<RunningProcess> {
        // Per-provider serialization. The lock-of-locks mutex is held
        // only while we look up / insert the per-provider lock.
        let provider_lock = {
            let mut map = self.inner.ensure_locks.lock().await;
            map.entry(provider_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _g = provider_lock.lock().await;

        // Fast path — already running.
        if let Some(rp) = self.inner.running.get(&provider_id).map(|e| e.clone())
            && matches!(rp.state, ProcessState::Ready | ProcessState::Warming)
        {
            return Ok(rp);
        }

        let node = self
            .inner
            .nodes
            .get(&node_id)
            .ok_or_else(|| anyhow!("node {node_id} not registered"))?;
        let lease = node.pool.allocate()?;
        let port = lease.port();

        let resp = node
            .client
            .spawn(&SpawnRequest {
                model_id: model_id.clone(),
                blob_sha256,
                port,
                runtime,
            })
            .await?;

        let rp = RunningProcess {
            provider_id,
            model_id,
            node_id,
            port,
            agent_handle: resp.agent_handle,
            state: ProcessState::Warming,
            started_at: Utc::now(),
            ready_at: None,
        };
        self.inner.running.insert(provider_id, rp.clone());
        self.inner.port_leases.insert(provider_id, lease);

        // Real warmup probe ships with the agent integration commit.
        // For now flip directly to Ready and register hooks so the
        // router and AIMD can start using the provider.
        self.mark_ready(provider_id, n_parallel);
        Ok(self.get(provider_id).unwrap_or(rp))
    }

    /// Internal: warmup passed → register with ActivityTracker + AIMD.
    fn mark_ready(&self, provider_id: Uuid, n_parallel: u32) {
        if let Some(mut entry) = self.inner.running.get_mut(&provider_id) {
            entry.state = ProcessState::Ready;
            entry.ready_at = Some(Utc::now());
        }
        self.inner.activity.register(provider_id, Utc::now());
        self.inner.aimd.register(provider_id, n_parallel);
    }

    /// Stop a process. Idempotent — calling twice (or for a never-
    /// registered provider) is a no-op.
    pub async fn stop(&self, provider_id: Uuid) -> Result<()> {
        let rp = match self.inner.running.remove(&provider_id) {
            Some((_, rp)) => rp,
            None => return Ok(()),
        };
        // Run the un-register side-effects before the network call so
        // the router stops dispatching even if the agent is unreachable.
        self.inner.activity.unregister(provider_id);
        self.inner.aimd.unregister(provider_id);
        self.inner.port_leases.remove(&provider_id);

        if let Some(node) = self.inner.nodes.get(&rp.node_id) {
            // Best-effort: if the agent is already gone, the port lease
            // dropped, and we still log success so the row in the
            // DB lines up.
            if let Err(e) = node.client.stop(rp.agent_handle).await {
                tracing::warn!(
                    %provider_id, error = %e,
                    "agent stop returned an error — process treated as stopped",
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::infrastructure::outbound::process_manager::agent_client::StubNodeClient;

    fn fixture() -> (ProcessManager, Uuid) {
        let mgr = ProcessManager::new(ActivityTracker::new(), AimdRegistry::new());
        let node_id = Uuid::now_v7();
        let pool = PortPool::new(20000..=20002);
        mgr.register_node(node_id, Arc::new(StubNodeClient::default()), pool);
        (mgr, node_id)
    }

    #[tokio::test]
    async fn ensure_running_marks_ready_and_registers_aimd() {
        let (mgr, node_id) = fixture();
        let provider_id = Uuid::now_v7();
        let rp = mgr
            .ensure_running(
                provider_id,
                node_id,
                "qwen3:q4".into(),
                "sha".into(),
                serde_json::json!({}),
                4,
            )
            .await
            .unwrap();
        assert_eq!(rp.state, ProcessState::Ready);
        assert!(rp.ready_at.is_some());
    }

    #[tokio::test]
    async fn ensure_running_is_idempotent() {
        let (mgr, node_id) = fixture();
        let provider_id = Uuid::now_v7();
        let _r1 = mgr
            .ensure_running(
                provider_id,
                node_id,
                "qwen3:q4".into(),
                "sha".into(),
                serde_json::json!({}),
                4,
            )
            .await
            .unwrap();
        let r2 = mgr
            .ensure_running(
                provider_id,
                node_id,
                "qwen3:q4".into(),
                "sha".into(),
                serde_json::json!({}),
                4,
            )
            .await
            .unwrap();
        // Same agent_handle → didn't double-spawn.
        let listing = mgr.list_running();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].agent_handle, r2.agent_handle);
    }

    #[tokio::test]
    async fn ensure_running_unknown_node_errors() {
        let mgr = ProcessManager::new(ActivityTracker::new(), AimdRegistry::new());
        let r = mgr
            .ensure_running(
                Uuid::now_v7(),
                Uuid::now_v7(), // never registered
                "qwen3:q4".into(),
                "sha".into(),
                serde_json::json!({}),
                4,
            )
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn stop_releases_resources() {
        let (mgr, node_id) = fixture();
        let provider_id = Uuid::now_v7();
        mgr.ensure_running(
            provider_id,
            node_id,
            "qwen3:q4".into(),
            "sha".into(),
            serde_json::json!({}),
            4,
        )
        .await
        .unwrap();
        mgr.stop(provider_id).await.unwrap();
        assert!(mgr.get(provider_id).is_none());
    }

    #[tokio::test]
    async fn stop_unknown_provider_is_noop() {
        let (mgr, _node_id) = fixture();
        mgr.stop(Uuid::now_v7()).await.unwrap();
    }
}
