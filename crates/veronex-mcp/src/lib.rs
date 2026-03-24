//! `veronex-mcp` — MCP protocol client library for Veronex.
//!
//! # Crate layout
//!
//! ```text
//! veronex-mcp
//! ├── types.rs           — McpTool, McpToolResult, McpToolCall, McpContent
//! ├── client.rs          — McpHttpClient  (Streamable HTTP 2025-03-26)
//! ├── session.rs         — McpSessionManager  (per-server session lifecycle)
//! ├── tool_cache.rs      — McpToolCache  (DashMap L1 + Valkey L2)
//! ├── result_cache.rs    — McpResultCache  (Valkey, SHA-256 keyed)
//! ├── circuit_breaker.rs — McpCircuitBreaker  (per-server state machine)
//! └── bin/
//!     └── weather.rs     — Standalone weather MCP server (example)
//! ```
//!
//! The bridge adapter (`McpBridgeAdapter`) that wires everything together
//! lives inside `veronex` itself because it depends on `AppState`,
//! `OllamaAdapter`, and `analytics_repo`.

pub mod circuit_breaker;
pub mod client;
pub mod result_cache;
pub mod session;
pub mod tool_cache;
pub mod types;

// ── Shared utilities ──────────────────────────────────────────────────────────

/// Truncate `s` to at most `max_len` bytes, always at a valid UTF-8 char boundary.
pub fn truncate_at_char_boundary(s: &mut String, max_len: usize) {
    if s.len() <= max_len {
        return;
    }
    let boundary = (0..=max_len).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
    s.truncate(boundary);
}

// Convenience re-exports
pub use client::McpHttpClient;
pub use session::McpSessionManager;
pub use tool_cache::McpToolCache;
pub use result_cache::McpResultCache;
pub use circuit_breaker::McpCircuitBreaker;
pub use types::{McpContent, McpTool, McpToolAnnotations, McpToolCall, McpToolResult};
