//! Phase 3 — `llm_nodes` repository port.
//!
//! The ProcessManager and admin handlers use this trait to manage compute
//! hosts. Hardware fields (gpu_model, total_vram_mb, etc.) are filled by an
//! initial agent `/probe` call and refreshed on demand.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::entities::{LlmNode, NodeProbeInfo};

#[async_trait]
pub trait LlmNodeRepository: Send + Sync {
    /// Insert a freshly-registered node. Caller is expected to have run the
    /// agent probe and supplied real hardware fields, but `probe_node` /
    /// `update_probe` can backfill them later.
    async fn register(&self, node: &LlmNode) -> Result<()>;

    /// Fetch a node by id. `None` when missing.
    async fn get(&self, id: Uuid) -> Result<Option<LlmNode>>;

    /// List every registered node. Sorted by `registered_at` for stable
    /// admin listings; the row count is bounded by the cluster size so a
    /// LIMIT isn't required.
    async fn list_all(&self) -> Result<Vec<LlmNode>>;

    /// Refresh the hardware-detail columns from a fresh probe. Sets
    /// `last_probe_at` and bumps `status` to `online` when not explicitly
    /// stopped — operators rely on the side-effect to confirm the node is
    /// answering.
    async fn update_probe(
        &self,
        id: Uuid,
        probe: &NodeProbeInfo,
        now: DateTime<Utc>,
    ) -> Result<bool>;

    /// Update only the status string. Used by the manager when a node
    /// becomes unreachable or the operator manually drains it.
    async fn update_status(&self, id: Uuid, status: &str) -> Result<bool>;

    /// Remove a node. Cascading FK on `llm_providers.node_id` clears the
    /// reference but does not delete provider rows — operator can keep the
    /// provider definition and point it at a new node.
    async fn delete(&self, id: Uuid) -> Result<bool>;
}
