//! SSOT for MCP-server queries used by both bootstrap and HTTP handlers.
//! Tool/discovery queries still live in the handler module — they are
//! tightly coupled with Valkey + Vespa side effects and would not benefit
//! from extraction.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ── Read DTOs ─────────────────────────────────────────────────────────

/// Connection target — minimal fields needed to (re)establish a session.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EnabledMcpServer {
    pub id: Uuid,
    pub slug: String,
    pub url: String,
    pub timeout_secs: i16,
}

/// Full row for the admin list/detail views.
#[derive(Debug, Clone)]
pub struct McpServerFullRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub url: String,
    pub is_enabled: bool,
    pub timeout_secs: i16,
    pub tool_count: i16,
    pub tools_summary: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for McpServerFullRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            slug: row.try_get("slug")?,
            url: row.try_get("url")?,
            is_enabled: row.try_get("is_enabled")?,
            timeout_secs: row.try_get("timeout_secs")?,
            tool_count: row.try_get("tool_count")?,
            tools_summary: row.try_get("tools_summary")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Identity row used by the agent-discovery endpoint and stats join.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct McpServerIdentity {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
}

const FULL_COLS: &str =
    "id, name, slug, url, is_enabled, timeout_secs, tool_count, tools_summary, created_at";

// ── Reads ─────────────────────────────────────────────────────────────

/// Every enabled MCP server, capped to a defensive bound. Returns an
/// empty vector on DB error — callers (bootstrap/reconcile) treat this
/// as best-effort.
pub async fn list_enabled_for_session(pool: &PgPool) -> Vec<EnabledMcpServer> {
    sqlx::query_as::<_, EnabledMcpServer>(
        "SELECT id, slug, url, timeout_secs FROM mcp_servers WHERE is_enabled = true LIMIT 500",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Every server in registration order. Errors propagate.
pub async fn list_full(pool: &PgPool) -> sqlx::Result<Vec<McpServerFullRow>> {
    let q = format!("SELECT {FULL_COLS} FROM mcp_servers ORDER BY created_at ASC LIMIT 500");
    sqlx::query_as::<_, McpServerFullRow>(&q).fetch_all(pool).await
}

/// Full row by id — used by patch/get-detail flows.
pub async fn get_full(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<McpServerFullRow>> {
    let q = format!("SELECT {FULL_COLS} FROM mcp_servers WHERE id = $1");
    sqlx::query_as::<_, McpServerFullRow>(&q)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Display name for audit logging on delete.
pub async fn get_name(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT name FROM mcp_servers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Slug uniqueness check excluding a specific id (the row being patched).
pub async fn slug_exists_excluding(
    pool: &PgPool,
    slug: &str,
    except_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE slug = $1 AND id != $2)")
        .bind(slug)
        .bind(except_id)
        .fetch_one(pool)
        .await
}

/// `(id, name, slug)` for every server — used to enrich Clickhouse stats.
pub async fn list_identities(pool: &PgPool) -> sqlx::Result<Vec<McpServerIdentity>> {
    sqlx::query_as::<_, McpServerIdentity>(
        "SELECT id, name, slug FROM mcp_servers ORDER BY name LIMIT 500",
    )
    .fetch_all(pool)
    .await
}

/// Single-server identity lookup (id, name, slug).
pub async fn get_identity(
    pool: &PgPool,
    id: Uuid,
) -> sqlx::Result<Option<McpServerIdentity>> {
    sqlx::query_as::<_, McpServerIdentity>(
        "SELECT id, name, slug FROM mcp_servers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

// ── Writes ────────────────────────────────────────────────────────────

/// Insert a new MCP server. Caller pre-allocates the id.
pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    slug: &str,
    url: &str,
    timeout_secs: i16,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO mcp_servers (id, name, slug, url, timeout_secs) VALUES ($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(name)
        .bind(slug)
        .bind(url)
        .bind(timeout_secs)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Patch the four mutable fields atomically, refresh `updated_at`.
pub async fn update_core(
    pool: &PgPool,
    id: Uuid,
    is_enabled: bool,
    url: &str,
    name: &str,
    slug: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE mcp_servers SET is_enabled = $1, url = $2, name = $3, slug = $4, updated_at = now() WHERE id = $5",
    )
    .bind(is_enabled)
    .bind(url)
    .bind(name)
    .bind(slug)
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Hard-delete. Tools cascade via FK.
pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
}

// ── Tool-side queries (mcp_server_tools) ──────────────────────────────

/// Tool snapshot row used by the patch-response payload.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct McpToolRow {
    pub tool_name: String,
    pub namespaced_name: String,
    pub description: Option<String>,
}

/// Per-server tool list, ordered by name. Capped defensively.
pub async fn list_tools(pool: &PgPool, server_id: Uuid) -> sqlx::Result<Vec<McpToolRow>> {
    sqlx::query_as::<_, McpToolRow>(
        "SELECT tool_name, namespaced_name, description FROM mcp_server_tools \
         WHERE server_id = $1 ORDER BY tool_name LIMIT 1000",
    )
    .bind(server_id)
    .fetch_all(pool)
    .await
}

/// Batch upsert of every tool a server discovered. Single round-trip via
/// `UNNEST` — caller pre-builds parallel arrays.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_tools_batch(
    pool: &PgPool,
    server_ids: &[Uuid],
    tool_names: &[&str],
    namespaced_names: &[String],
    descriptions: &[&str],
    schemas: &[serde_json::Value],
    count: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO mcp_server_tools (server_id, tool_name, namespaced_name, description, input_schema, discovered_at)
         SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[], $4::text[], $5::jsonb[], array_fill(now()::timestamptz, ARRAY[$6::int]))
         ON CONFLICT (server_id, tool_name) DO UPDATE
           SET namespaced_name = EXCLUDED.namespaced_name,
               description     = EXCLUDED.description,
               input_schema    = EXCLUDED.input_schema,
               discovered_at   = EXCLUDED.discovered_at",
    )
    .bind(server_ids)
    .bind(tool_names)
    .bind(namespaced_names)
    .bind(descriptions)
    .bind(schemas)
    .bind(count)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Refresh the denormalized `tool_count` + `tools_summary` columns.
pub async fn update_tools_summary(
    pool: &PgPool,
    server_id: Uuid,
    tool_count: i16,
    summary: &serde_json::Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE mcp_servers SET tool_count = $1, tools_summary = $2, updated_at = now() WHERE id = $3",
    )
    .bind(tool_count)
    .bind(summary)
    .bind(server_id)
    .execute(pool)
    .await
    .map(|_| ())
}
