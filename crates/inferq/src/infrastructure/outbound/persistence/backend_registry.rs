use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::llm_backend_registry::LlmBackendRegistry;
use crate::domain::entities::LlmBackend;
use crate::domain::enums::{BackendType, LlmBackendStatus};

pub struct PostgresBackendRegistry {
    pool: PgPool,
}

impl PostgresBackendRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── Enum conversions ───────────────────────────────────────────────────────────

fn backend_type_to_str(b: &BackendType) -> &'static str {
    match b {
        BackendType::Ollama => "ollama",
        BackendType::Gemini => "gemini",
    }
}

fn str_to_backend_type(s: &str) -> Result<BackendType> {
    match s {
        "ollama" => Ok(BackendType::Ollama),
        "gemini" => Ok(BackendType::Gemini),
        _ => Err(anyhow::anyhow!("unknown backend type: {s}")),
    }
}

fn status_to_str(s: &LlmBackendStatus) -> &'static str {
    match s {
        LlmBackendStatus::Online => "online",
        LlmBackendStatus::Offline => "offline",
        LlmBackendStatus::Degraded => "degraded",
    }
}

fn str_to_status(s: &str) -> LlmBackendStatus {
    match s {
        "online" => LlmBackendStatus::Online,
        "degraded" => LlmBackendStatus::Degraded,
        _ => LlmBackendStatus::Offline,
    }
}

// ── Row mapping ────────────────────────────────────────────────────────────────

fn row_to_backend(row: &sqlx::postgres::PgRow) -> Result<LlmBackend> {
    use sqlx::Row as _;

    let id: Uuid = row.try_get("id").context("id")?;
    let name: String = row.try_get("name").context("name")?;
    let backend_type_str: String = row.try_get("backend_type").context("backend_type")?;
    let url: String = row.try_get("url").context("url")?;
    let api_key_encrypted: Option<String> = row.try_get("api_key_encrypted").context("api_key_encrypted")?;
    let is_active: bool = row.try_get("is_active").context("is_active")?;
    let total_vram_mb: i64 = row.try_get("total_vram_mb").context("total_vram_mb")?;
    let gpu_index: Option<i16> = row.try_get("gpu_index").context("gpu_index")?;
    let server_id: Option<Uuid> = row.try_get("server_id").context("server_id")?;
    let agent_url: Option<String> = row.try_get("agent_url").context("agent_url")?;
    let status_str: String = row.try_get("status").context("status")?;
    let registered_at: DateTime<Utc> = row.try_get("registered_at").context("registered_at")?;

    Ok(LlmBackend {
        id,
        name,
        backend_type: str_to_backend_type(&backend_type_str)?,
        url,
        api_key_encrypted,
        is_active,
        total_vram_mb,
        gpu_index,
        server_id,
        agent_url,
        status: str_to_status(&status_str),
        registered_at,
    })
}

// ── Repository impl ────────────────────────────────────────────────────────────

#[async_trait]
impl LlmBackendRegistry for PostgresBackendRegistry {
    async fn register(&self, backend: &LlmBackend) -> Result<()> {
        sqlx::query(
            "INSERT INTO llm_backends
                 (id, name, backend_type, url, api_key_encrypted, is_active,
                  total_vram_mb, gpu_index, server_id, agent_url,
                  status, registered_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(backend.id)
        .bind(&backend.name)
        .bind(backend_type_to_str(&backend.backend_type))
        .bind(&backend.url)
        .bind(&backend.api_key_encrypted)
        .bind(backend.is_active)
        .bind(backend.total_vram_mb)
        .bind(backend.gpu_index)
        .bind(backend.server_id)
        .bind(&backend.agent_url)
        .bind(status_to_str(&backend.status))
        .bind(backend.registered_at)
        .execute(&self.pool)
        .await
        .context("failed to register backend")?;

        Ok(())
    }

    async fn list_active(&self) -> Result<Vec<LlmBackend>> {
        let rows = sqlx::query(
            "SELECT id, name, backend_type, url, api_key_encrypted, is_active, total_vram_mb, gpu_index, server_id, agent_url, status, registered_at
             FROM llm_backends
             WHERE is_active = true AND status = 'online'
             ORDER BY registered_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list active backends")?;

        rows.iter().map(|r| row_to_backend(r)).collect()
    }

    async fn list_all(&self) -> Result<Vec<LlmBackend>> {
        let rows = sqlx::query(
            "SELECT id, name, backend_type, url, api_key_encrypted, is_active, total_vram_mb, gpu_index, server_id, agent_url, status, registered_at
             FROM llm_backends
             ORDER BY registered_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list all backends")?;

        rows.iter().map(|r| row_to_backend(r)).collect()
    }

    async fn get(&self, id: Uuid) -> Result<Option<LlmBackend>> {
        let row = sqlx::query(
            "SELECT id, name, backend_type, url, api_key_encrypted, is_active, total_vram_mb, gpu_index, server_id, agent_url, status, registered_at
             FROM llm_backends
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get backend")?;

        match row {
            Some(r) => Ok(Some(row_to_backend(&r)?)),
            None => Ok(None),
        }
    }

    async fn update_status(&self, id: Uuid, status: LlmBackendStatus) -> Result<()> {
        sqlx::query("UPDATE llm_backends SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status_to_str(&status))
            .execute(&self.pool)
            .await
            .context("failed to update backend status")?;

        Ok(())
    }

    async fn deactivate(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM llm_backends WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("failed to delete backend")?;

        Ok(())
    }

    async fn update(&self, backend: &LlmBackend) -> Result<()> {
        sqlx::query(
            "UPDATE llm_backends
             SET name = $1,
                 url = $2,
                 api_key_encrypted = COALESCE($3, api_key_encrypted),
                 total_vram_mb = $4,
                 gpu_index = $5,
                 server_id = $6,
                 agent_url = $7
             WHERE id = $8",
        )
        .bind(&backend.name)
        .bind(&backend.url)
        .bind(&backend.api_key_encrypted)
        .bind(backend.total_vram_mb)
        .bind(backend.gpu_index)
        .bind(backend.server_id)
        .bind(&backend.agent_url)
        .bind(backend.id)
        .execute(&self.pool)
        .await
        .context("failed to update backend")?;

        Ok(())
    }
}
