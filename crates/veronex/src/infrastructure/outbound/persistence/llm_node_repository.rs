//! Postgres implementation of [`LlmNodeRepository`] (Phase 3).
//!
//! Plain CRUD — no encryption, no caching. Admin endpoints use this directly;
//! the ProcessManager keeps an in-memory snapshot it refreshes on probe.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::llm_node_repository::LlmNodeRepository;
use crate::domain::entities::{
    DeploymentKind, GpuAccel, HostArch, HostOs, LlmNode, NodeProbeInfo,
};

const NODE_COLS: &str = "id, hostname, deployment_kind, os, arch, gpu_accel, gpu_model, total_vram_mb, total_ram_mb, cpu_threads, agent_url, status, registered_at, last_probe_at";

pub struct PostgresLlmNodeRepository {
    pool: PgPool,
}

impl PostgresLlmNodeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_node(row: &sqlx::postgres::PgRow) -> Result<LlmNode> {
    use sqlx::Row as _;
    let id: Uuid = row.try_get("id").context("id")?;
    let hostname: String = row.try_get("hostname").context("hostname")?;
    let deployment_kind_str: String = row
        .try_get("deployment_kind")
        .context("deployment_kind")?;
    let os_str: String = row.try_get("os").context("os")?;
    let arch_str: String = row.try_get("arch").context("arch")?;
    let gpu_accel_str: String = row.try_get("gpu_accel").context("gpu_accel")?;
    let gpu_model: Option<String> = row.try_get("gpu_model").context("gpu_model")?;
    let total_vram_mb: i64 = row.try_get("total_vram_mb").context("total_vram_mb")?;
    let total_ram_mb: i64 = row.try_get("total_ram_mb").context("total_ram_mb")?;
    let cpu_threads: i16 = row.try_get("cpu_threads").context("cpu_threads")?;
    let agent_url: String = row.try_get("agent_url").context("agent_url")?;
    let status: String = row.try_get("status").context("status")?;
    let registered_at: DateTime<Utc> = row.try_get("registered_at").context("registered_at")?;
    let last_probe_at: Option<DateTime<Utc>> = row
        .try_get("last_probe_at")
        .context("last_probe_at")?;

    Ok(LlmNode {
        id,
        hostname,
        deployment_kind: deployment_kind_str
            .parse::<DeploymentKind>()
            .map_err(|e| anyhow::anyhow!(e))?,
        os: os_str.parse::<HostOs>().map_err(|e| anyhow::anyhow!(e))?,
        arch: arch_str.parse::<HostArch>().map_err(|e| anyhow::anyhow!(e))?,
        gpu_accel: gpu_accel_str
            .parse::<GpuAccel>()
            .map_err(|e| anyhow::anyhow!(e))?,
        gpu_model,
        total_vram_mb,
        total_ram_mb,
        cpu_threads,
        agent_url,
        status,
        registered_at,
        last_probe_at,
    })
}

#[async_trait]
impl LlmNodeRepository for PostgresLlmNodeRepository {
    async fn register(&self, node: &LlmNode) -> Result<()> {
        sqlx::query(
            "INSERT INTO llm_nodes
                 (id, hostname, deployment_kind, os, arch, gpu_accel, gpu_model,
                  total_vram_mb, total_ram_mb, cpu_threads, agent_url, status,
                  registered_at, last_probe_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(node.id)
        .bind(&node.hostname)
        .bind(node.deployment_kind.as_str())
        .bind(node.os.as_str())
        .bind(node.arch.as_str())
        .bind(node.gpu_accel.as_str())
        .bind(&node.gpu_model)
        .bind(node.total_vram_mb)
        .bind(node.total_ram_mb)
        .bind(node.cpu_threads)
        .bind(&node.agent_url)
        .bind(&node.status)
        .bind(node.registered_at)
        .bind(node.last_probe_at)
        .execute(&self.pool)
        .await
        .context("register llm_node")?;
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<LlmNode>> {
        let q = format!("SELECT {NODE_COLS} FROM llm_nodes WHERE id = $1");
        let row = sqlx::query(&q).bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_node).transpose()
    }

    async fn list_all(&self) -> Result<Vec<LlmNode>> {
        let q = format!("SELECT {NODE_COLS} FROM llm_nodes ORDER BY registered_at ASC");
        let rows = sqlx::query(&q).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_node).collect()
    }

    async fn update_probe(
        &self,
        id: Uuid,
        probe: &NodeProbeInfo,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE llm_nodes SET
                 os = $2,
                 arch = $3,
                 gpu_accel = $4,
                 gpu_model = $5,
                 total_vram_mb = $6,
                 total_ram_mb = $7,
                 cpu_threads = $8,
                 last_probe_at = $9,
                 status = CASE WHEN status = 'stopped' THEN status ELSE 'online' END
             WHERE id = $1",
        )
        .bind(id)
        .bind(probe.os.as_str())
        .bind(probe.arch.as_str())
        .bind(probe.gpu_accel.as_str())
        .bind(&probe.gpu_model)
        .bind(probe.total_vram_mb)
        .bind(probe.total_ram_mb)
        .bind(probe.cpu_threads)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("update_probe")?;
        Ok(r.rows_affected() > 0)
    }

    async fn update_status(&self, id: Uuid, status: &str) -> Result<bool> {
        let r = sqlx::query("UPDATE llm_nodes SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(&self.pool)
            .await
            .context("update_status")?;
        Ok(r.rows_affected() > 0)
    }

    async fn delete(&self, id: Uuid) -> Result<bool> {
        let r = sqlx::query("DELETE FROM llm_nodes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("delete llm_node")?;
        Ok(r.rows_affected() > 0)
    }
}
