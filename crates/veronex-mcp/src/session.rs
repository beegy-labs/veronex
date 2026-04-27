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

/// True when the error chain points at a reqwest transport-level failure
/// (DNS lookup, TCP connect, TLS handshake, broken pipe, idle-pool stale
/// connection that was reset by the upstream gateway, etc.). These deserve
/// a one-shot session re-init + retry just like a 404 — the session URL
/// is still correct, only the underlying TCP connection died.
///
/// The detection is by string match on the error chain because `anyhow`
/// erases the source type. reqwest's `Display` on these error classes
/// contains stable phrases — verified across 0.11.x / 0.12.x.
pub(crate) fn is_transport_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let s = cause.to_string();
        s.contains("error sending request")
            || s.contains("connection closed")
            || s.contains("connection reset")
            || s.contains("broken pipe")
            || s.contains("operation timed out")
            || s.contains("dns error")
    })
}

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
    /// If the call returns a session-expired error OR a reqwest transport
    /// error (TCP reset, DNS hiccup, idle-pool stale connection killed by
    /// the upstream gateway), the session is re-initialized and the call
    /// is retried once.
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
            .as_deref()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No session for server {server_id}"))?;

        let client = Arc::clone(&self.client);

        match f(Arc::clone(&client), entry.session.clone()).await {
            Ok(v) => Ok(v),
            Err(e) if e.to_string().contains(SESSION_EXPIRED_MARKER)
                || is_transport_error(&e) =>
            {
                let reason = if e.to_string().contains(SESSION_EXPIRED_MARKER) {
                    "session expired (404)"
                } else {
                    "transport error"
                };
                warn!(
                    server_id = %server_id,
                    reason,
                    error = %e,
                    "MCP call failed — re-initializing session and retrying once"
                );

                // Acquire per-server lock before re-init. Concurrent tasks that hit 404 at
                // the same time will queue here; only the first does the actual re-init.
                // Clone the Arc before the await — never hold a DashMap Ref across .await.
                let lock = self.reinit_locks
                    .entry(server_id)
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                    .clone();
                let _guard = lock.lock().await;

                // Check if another task already re-initialized while we waited.
                // Detect by comparing session_id: if it changed, re-init already happened.
                // For stateless servers (session_id = None), always re-init — 404 means
                // the server restarted and the handshake must be replayed.
                let session_changed = self.sessions
                    .get(&server_id)
                    .map(|e| e.session.session_id != entry.session.session_id)
                    .unwrap_or(false);

                if !session_changed {
                    self.sessions.remove(&server_id);
                    self.connect(server_id, &entry.session.server_name, &entry.session.url, entry.session.timeout_secs).await?;
                }

                let fresh = self.sessions
                    .get(&server_id)
                    .map(|e| e.session.clone())
                    .ok_or_else(|| anyhow::anyhow!("Re-init succeeded but session missing"))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transport_error_matches_reqwest_phrases() {
        // Top-level reqwest::Error::Display string for a connect failure.
        let e = anyhow::anyhow!("error sending request for url (https://x/)");
        assert!(is_transport_error(&e));
    }

    #[test]
    fn is_transport_error_matches_dns_in_chain() {
        let inner = std::io::Error::other("dns error: NXDOMAIN");
        let e: anyhow::Error = anyhow::Error::new(inner).context("MCP call");
        assert!(is_transport_error(&e));
    }

    #[test]
    fn is_transport_error_skips_protocol_errors() {
        // Logical / protocol errors should NOT trigger a transport retry —
        // those are real failures the caller must surface.
        let e = anyhow::anyhow!("MCP tool returned schema-validation error");
        assert!(!is_transport_error(&e));
    }

    #[test]
    fn is_transport_error_does_not_fire_on_session_expired() {
        // Session-expired (404) has its own retry path; transport-error
        // detection must not also fire for it (avoids double-retry).
        let e = anyhow::anyhow!("MCP {SESSION_EXPIRED_MARKER} (404) for https://x/");
        assert!(!is_transport_error(&e));
    }
}
