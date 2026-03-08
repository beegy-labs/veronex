use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::gpu_server_registry::GpuServerRegistry;
use crate::domain::entities::GpuServer;

pub struct PostgresGpuServerRegistry {
    pool: PgPool,
}

impl PostgresGpuServerRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_server(row: &sqlx::postgres::PgRow) -> Result<GpuServer> {
    use sqlx::Row as _;

    let id: Uuid = row.try_get("id").context("id")?;
    let name: String = row.try_get("name").context("name")?;
    let node_exporter_url: Option<String> =
        row.try_get("node_exporter_url").context("node_exporter_url")?;
    let registered_at: DateTime<Utc> = row.try_get("registered_at").context("registered_at")?;

    Ok(GpuServer {
        id,
        name,
        node_exporter_url,
        registered_at,
    })
}

#[async_trait]
impl GpuServerRegistry for PostgresGpuServerRegistry {
    async fn register(&self, server: GpuServer) -> Result<()> {
        sqlx::query(
            "INSERT INTO gpu_servers (id, name, node_exporter_url, registered_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(server.id)
        .bind(&server.name)
        .bind(&server.node_exporter_url)
        .bind(server.registered_at)
        .execute(&self.pool)
        .await
        .context("failed to register gpu server")?;

        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<GpuServer>> {
        let rows = sqlx::query(
            "SELECT id, name, node_exporter_url, registered_at
             FROM gpu_servers
             ORDER BY registered_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list gpu servers")?;

        rows.iter().map(row_to_server).collect()
    }

    async fn get(&self, id: Uuid) -> Result<Option<GpuServer>> {
        let row = sqlx::query(
            "SELECT id, name, node_exporter_url, registered_at
             FROM gpu_servers
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get gpu server")?;

        match row {
            Some(r) => Ok(Some(row_to_server(&r)?)),
            None => Ok(None),
        }
    }

    async fn update(&self, server: &GpuServer) -> Result<()> {
        sqlx::query(
            "UPDATE gpu_servers SET name = $2, node_exporter_url = $3 WHERE id = $1",
        )
        .bind(server.id)
        .bind(&server.name)
        .bind(&server.node_exporter_url)
        .execute(&self.pool)
        .await
        .context("failed to update gpu server")?;

        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM gpu_servers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to delete gpu server")?;

        Ok(())
    }
}
