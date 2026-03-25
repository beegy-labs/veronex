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
}

// ── Manager ───────────────────────────────────────────────────────────────────

/// Thread-safe session store with automatic re-initialization on expiry.
pub struct McpSessionManager {
    sessions: Arc<DashMap<Uuid, SessionEntry>>,
    /// Shared client — reuse connection pool across all sessions and retries.
    client: Arc<McpHttpClient>,
    /// Per-server mutex for re-initialization. Prevents concurrent 404 handlers from
    /// racing to re-initialize the same server and sending N duplicate handshakes.
    reinit_locks: Arc<DashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>,
}

impl McpSessionManager {
    pub fn new(client: McpHttpClient) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            client: Arc::new(client),
            reinit_locks: Arc::new(DashMap::new()),
        }
    }

    /// Initialize a session for the given server and store it.
    pub async fn connect(
        &self,
        server_id: Uuid,
        server_name: impl Into<String>,
        url: impl Into<String>,
        timeout_secs: u16,
    ) -> Result<()> {
        let server_name = server_name.into();
        let url = url.into();

        let mut session = self
            .client
            .initialize(server_id, &server_name, &url)
            .await?;
        session.timeout_secs = timeout_secs;

        info!(
            server_id = %server_id,
            server_name = %server_name,
            session_id = ?session.session_id,
            "MCP session established"
        );

        self.sessions.insert(server_id, SessionEntry { session });
        Ok(())
    }

    /// Remove a session (e.g. server went offline).
    pub fn disconnect(&self, server_id: Uuid) {
        self.sessions.remove(&server_id);
        self.reinit_locks.remove(&server_id);
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

                // Acquire per-server lock before re-init. Concurrent tasks that hit 404 at
                // the same time will queue here; only the first does the actual re-init.
                // Clone the Arc before the await — never hold a DashMap Ref across .await.
                let lock = self.reinit_locks
                    .entry(server_id)
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                    .clone();
                let _guard = lock.lock().await;

                // Check if another task already re-initialized while we waited.
                let fresh = if let Some(e) = self.sessions.get(&server_id) {
                    e.session.clone()
                } else {
                    // We hold the lock and no session exists — do the re-init.
                    self.connect(server_id, &entry.session.server_name, &entry.session.url, entry.session.timeout_secs).await?;
                    self.sessions
                        .get(&server_id)
                        .map(|e| e.session.clone())
                        .ok_or_else(|| anyhow::anyhow!("Re-init succeeded but session missing"))?
                };

                f(client, fresh).await
            }
            Err(e) => Err(e),
        }
    }

    /// Check liveness of all connected servers (parallel).
    pub async fn ping_all(&self) -> Vec<(Uuid, bool)> {
        // Snapshot to avoid holding DashMap Refs across .await (shard lock violation).
        let entries: Vec<(Uuid, McpSession)> = self
            .sessions
            .iter()
            .map(|e| (*e.key(), e.value().session.clone()))
            .collect();

        let client = Arc::clone(&self.client);
        let futs = entries.into_iter().map(|(id, session)| {
            let client = Arc::clone(&client);
            async move {
                let alive = client.ping(&session).await.is_ok();
                (id, alive)
            }
        });
        futures::future::join_all(futs).await
    }

    /// Returns IDs of all currently tracked servers.
    pub fn server_ids(&self) -> Vec<Uuid> {
        self.sessions.iter().map(|e| *e.key()).collect()
    }

    /// Returns the per-server tool-call timeout. Falls back to 30 s if the
    /// server has no active session (e.g. circuit open, offline).
    pub fn get_timeout_secs(&self, server_id: Uuid) -> u16 {
        self.sessions
            .get(&server_id)
            .map(|e| e.session.timeout_secs)
            .unwrap_or(30)
    }

    /// O(1) check — true when at least one session is active.
    /// Prefer this over `!server_ids().is_empty()` on hot paths.
    pub fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
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
