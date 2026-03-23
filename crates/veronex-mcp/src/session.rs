//! `McpSessionManager` — per-server MCP session lifecycle.
//!
//! Each Veronex replica holds its own sessions (Mcp-Session-Id is not shared).
//! On 404 (session expired), the manager transparently re-initializes.

use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;
use tracing::{info, warn};
use uuid::Uuid;

use crate::client::{McpHttpClient, McpSession};
use crate::types::McpToolResult;

/// Sentinel substring in error messages that signals a 404 session-expired condition.
/// `McpHttpClient::send` produces this string; `with_session` matches it for re-init.
/// Using a const prevents silent breakage if the message text changes in either place.
pub(crate) const SESSION_EXPIRED_MARKER: &str = "session expired";

// ── Session entry ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SessionEntry {
    session: McpSession,
    server_name: String,
    url: String,
}

// ── Manager ───────────────────────────────────────────────────────────────────

/// Thread-safe session store with automatic re-initialization on expiry.
pub struct McpSessionManager {
    sessions: Arc<DashMap<Uuid, SessionEntry>>,
    /// Shared client — reuse connection pool across all sessions and retries.
    client: Arc<McpHttpClient>,
}

impl McpSessionManager {
    pub fn new(client: McpHttpClient) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            client: Arc::new(client),
        }
    }

    /// Initialize a session for the given server and store it.
    pub async fn connect(
        &self,
        server_id: Uuid,
        server_name: impl Into<String>,
        url: impl Into<String>,
    ) -> Result<()> {
        let server_name = server_name.into();
        let url = url.into();

        let session = self
            .client
            .initialize(server_id, &server_name, &url)
            .await?;

        info!(
            server_id = %server_id,
            server_name = %server_name,
            session_id = ?session.session_id,
            "MCP session established"
        );

        self.sessions.insert(
            server_id,
            SessionEntry { session, server_name, url },
        );
        Ok(())
    }

    /// Remove a session (e.g. server went offline).
    pub fn disconnect(&self, server_id: Uuid) {
        self.sessions.remove(&server_id);
    }

    /// Get the active session, re-initializing transparently on 404.
    ///
    /// `f` receives `(&McpHttpClient, &McpSession)` and returns a `Result<T>`.
    /// If the call returns a session-expired error, the session is re-initialized
    /// and the call is retried once.
    pub async fn with_session<F, Fut, T>(
        &self,
        server_id: Uuid,
        f: F,
    ) -> Result<T>
    where
        F: Fn(Arc<McpHttpClient>, McpSession) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let entry = self
            .sessions
            .get(&server_id)
            .map(|e| e.clone())
            .ok_or_else(|| anyhow::anyhow!("No session for server {server_id}"))?;

        let client = Arc::clone(&self.client);

        match f(Arc::clone(&client), entry.session.clone()).await {
            Ok(v) => Ok(v),
            Err(e) if e.to_string().contains(SESSION_EXPIRED_MARKER) => {
                warn!(server_id = %server_id, "MCP session expired — re-initializing");
                // Remove stale session and re-init WITHOUT the old session-id header
                self.sessions.remove(&server_id);
                self.connect(server_id, &entry.server_name, &entry.url).await?;

                let fresh = self
                    .sessions
                    .get(&server_id)
                    .map(|e| e.session.clone())
                    .ok_or_else(|| anyhow::anyhow!("Re-init succeeded but session missing"))?;

                f(client, fresh).await
            }
            Err(e) => Err(e),
        }
    }

    /// Check liveness of all connected servers.
    pub async fn ping_all(&self) -> Vec<(Uuid, bool)> {
        // Snapshot to avoid holding DashMap Refs across .await (shard lock violation).
        let entries: Vec<(Uuid, McpSession)> = self
            .sessions
            .iter()
            .map(|e| (*e.key(), e.value().session.clone()))
            .collect();

        let mut results = Vec::with_capacity(entries.len());
        for (id, session) in entries {
            let alive = self.client.ping(&session).await.is_ok();
            results.push((id, alive));
        }
        results
    }

    /// Returns IDs of all currently tracked servers.
    pub fn server_ids(&self) -> Vec<Uuid> {
        self.sessions.iter().map(|e| *e.key()).collect()
    }

    /// Convenience: call a tool on a server by ID.
    ///
    /// Handles session re-initialization on 404 transparently via `with_session`.
    pub async fn call_tool(
        &self,
        server_id: Uuid,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult> {
        let tool_name = tool_name.to_string();
        self.with_session(server_id, move |client, session| {
            let tool_name = tool_name.clone();
            let arguments = arguments.clone();
            async move { client.call_tool(&session, &tool_name, &arguments).await }
        })
        .await
    }
}
