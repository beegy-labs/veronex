//! `McpHttpClient` — Streamable HTTP MCP client (spec: 2025-03-26).
//!
//! Handles the initialize handshake, ping, tools/list, and tools/call.
//! Session management (Mcp-Session-Id lifecycle) is handled by [`McpSessionManager`].

use std::time::{Duration, Instant};

/// Default HTTP client timeout for MCP connections.
/// This covers the initialize handshake, ping, and list_tools calls.
/// Individual tool call timeouts are enforced by the bridge (30 s).
const MCP_CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

use anyhow::{anyhow, Result};
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

use crate::session::SESSION_EXPIRED_MARKER;
use crate::types::{McpContent, McpTool, McpToolResult};

/// Max bytes for a tool description string. Prevents oversized context injection.
const MAX_TOOL_DESCRIPTION_BYTES: usize = 4_096;
/// Max serialized bytes for a tool inputSchema. Prevents deeply-nested JSON bombs.
const MAX_TOOL_SCHEMA_BYTES: usize = 16_384;

// ── Session ───────────────────────────────────────────────────────────────────

/// Active MCP session for one server.
#[derive(Debug, Clone)]
pub struct McpSession {
    pub server_id: Uuid,
    /// Base MCP endpoint, e.g. `http://weather-mcp:3000/mcp`.
    pub url: String,
    /// Assigned by the server in the `Initialize` response header.
    /// `None` for servers that omit the header (stateless servers).
    pub session_id: Option<String>,
    /// Short slug used for tool namespacing, e.g. `weather`.
    pub server_name: String,
}

// ── Client ────────────────────────────────────────────────────────────────────

pub struct McpHttpClient {
    inner: reqwest::Client,
}

impl Default for McpHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl McpHttpClient {
    pub fn new() -> Self {
        let inner = reqwest::Client::builder()
            .timeout(MCP_CLIENT_TIMEOUT)
            .build()
            .expect("McpHttpClient: reqwest build failed");
        Self { inner }
    }

    // ── Initialize ────────────────────────────────────────────────────────────

    /// Perform the MCP initialize handshake and return an active session.
    ///
    /// Intentionally omits `sampling` from capabilities — if declared but
    /// unimplemented the MCP server will hang waiting for a response.
    pub async fn initialize(
        &self,
        server_id: Uuid,
        server_name: impl Into<String>,
        url: impl Into<String>,
    ) -> Result<McpSession> {
        let url = url.into();
        let server_name = server_name.into();

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "roots": { "listChanged": false }
                    // sampling omitted: v1 does not implement reverse LLM calls
                },
                "clientInfo": {
                    "name": "veronex",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        let resp = self
            .inner
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&body)
            .send()
            .await?;

        // Extract Mcp-Session-Id before consuming the body
        let session_id = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        let result: serde_json::Value = resp.json().await?;
        if let Some(err) = result.get("error") {
            return Err(anyhow!("MCP initialize error from {url}: {err:?}"));
        }

        // Send `notifications/initialized` (fire-and-forget, no response expected)
        let notif = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let mut req = self.inner.post(&url).json(&notif);
        if let Some(ref sid) = session_id {
            req = req.header("mcp-session-id", sid);
        }
        if let Err(e) = req.send().await {
            warn!("McpHttpClient: failed to send initialized notification to {url}: {e}");
        }

        Ok(McpSession { server_id, url, session_id, server_name })
    }

    // ── Ping ──────────────────────────────────────────────────────────────────

    /// Application-level liveness check (JSON-RPC `ping`).
    /// A live TCP connection does not guarantee the JSON-RPC stack is healthy.
    pub async fn ping(&self, session: &McpSession) -> Result<()> {
        let body = json!({ "jsonrpc": "2.0", "id": 99, "method": "ping" });
        let resp = self.send(session, &body).await?;
        let v: serde_json::Value = resp.json().await?;
        if v.get("error").is_some() {
            return Err(anyhow!("ping failed for {}: {:?}", session.url, v["error"]));
        }
        Ok(())
    }

    // ── tools/list ────────────────────────────────────────────────────────────

    /// Fetch the tool list and enrich each tool with server metadata.
    pub async fn list_tools(&self, session: &McpSession) -> Result<Vec<McpTool>> {
        let body = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
        let resp = self.send(session, &body).await?;
        let v: serde_json::Value = resp.json().await?;

        if let Some(err) = v.get("error") {
            return Err(anyhow!(
                "tools/list error from {}: {err:?}",
                session.url
            ));
        }

        let raw_tools = v["result"]["tools"]
            .as_array()
            .ok_or_else(|| anyhow!("tools/list: missing `tools` array"))?;

        let mut tools = Vec::with_capacity(raw_tools.len());
        for t in raw_tools {
            let mut tool: McpTool = serde_json::from_value(t.clone()).unwrap_or_else(|_| {
                McpTool {
                    name: t["name"].as_str().unwrap_or("unknown").to_owned(),
                    description: t["description"].as_str().unwrap_or("").to_owned(),
                    input_schema: t["inputSchema"].clone(),
                    annotations: Default::default(),
                    server_id: session.server_id,
                    server_name: session.server_name.clone(),
                }
            });
            tool.server_id = session.server_id;
            tool.server_name = session.server_name.clone();

            // Guard against oversized fields from malicious/misconfigured servers.
            if tool.description.len() > MAX_TOOL_DESCRIPTION_BYTES {
                warn!(tool = %tool.name, "MCP tool description truncated");
                tool.description.truncate(MAX_TOOL_DESCRIPTION_BYTES);
            }
            let schema_bytes = serde_json::to_string(&tool.input_schema)
                .map(|s| s.len())
                .unwrap_or(usize::MAX);
            if schema_bytes > MAX_TOOL_SCHEMA_BYTES {
                return Err(anyhow!(
                    "tool {} inputSchema too large ({schema_bytes} bytes > {MAX_TOOL_SCHEMA_BYTES})",
                    tool.name
                ));
            }

            tools.push(tool);
        }

        Ok(tools)
    }

    // ── tools/call ────────────────────────────────────────────────────────────

    /// Execute a tool on the MCP server.
    ///
    /// Returns `Ok(result)` for both successes and logical tool errors (`isError: true`).
    /// Returns `Err` only for protocol-level failures (JSON-RPC error field present).
    pub async fn call_tool(
        &self,
        session: &McpSession,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<McpToolResult> {
        let start = Instant::now();

        let body = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        let resp = self.send(session, &body).await?;
        let v: serde_json::Value = resp.json().await?;
        let latency_ms = start.elapsed().as_millis() as u32;

        // Protocol error (different from tool execution error)
        if let Some(err) = v.get("error") {
            return Err(anyhow!(
                "MCP protocol error from {}/{}: {err:?}",
                session.url,
                tool_name
            ));
        }

        let result = &v["result"];
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let content: Vec<McpContent> = result["content"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| serde_json::from_value(c.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(McpToolResult { content, is_error, latency_ms, from_cache: false })
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn send(
        &self,
        session: &McpSession,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let mut req = self.inner.post(&session.url).json(body);
        if let Some(ref sid) = session.session_id {
            req = req.header("mcp-session-id", sid);
        }
        let resp = req.send().await?;

        // 404 = session expired. Caller (McpSessionManager) must re-initialize.
        // Error message must contain SESSION_EXPIRED_MARKER so with_session can detect it.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(anyhow!("MCP {SESSION_EXPIRED_MARKER} (404) for {}", session.url));
        }

        Ok(resp)
    }
}
