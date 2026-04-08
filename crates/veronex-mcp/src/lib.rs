//! `veronex-mcp` — MCP protocol client library for Veronex.
//!
//! # Crate layout
//!
//! ```text
//! veronex-mcp
//! ├── types.rs           — McpTool, McpToolResult, McpToolCall, McpContent
//! ├── client.rs          — McpHttpClient  (Streamable HTTP 2025-03-26)
//! ├── session.rs         — McpSessionManager  (per-server session lifecycle, per-server timeout)
//! ├── tool_cache.rs      — McpToolCache  (DashMap L1 + Valkey L2, ACL filter)
//! ├── result_cache.rs    — McpResultCache  (Valkey, SHA-256 keyed)
//! ├── circuit_breaker.rs — McpCircuitBreaker  (per-server state machine)
//! ├── tools/
//! │   ├── mod.rs         — Tool trait (spec + call)
//! │   └── weather.rs     — WeatherTool  (get_weather, L1/L2 cache, singleflight)
//! └── bin/
//!     └── veronex-mcp.rs — Unified MCP server  (Vec<Arc<dyn Tool>>, O(1) dispatch)
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
pub mod vector;

// Server-side modules (used by veronex-mcp binary)
pub mod geo;
pub mod tools;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_no_op_within_limit() {
        let mut s = "hello".to_string();
        truncate_at_char_boundary(&mut s, 10);
        assert_eq!(s, "hello");
    }

    #[test]
    fn truncate_ascii_at_byte_boundary() {
        let mut s = "hello world".to_string();
        truncate_at_char_boundary(&mut s, 5);
        assert_eq!(s, "hello");
    }

    #[test]
    fn truncate_multibyte_backs_off_to_boundary() {
        // "가" = 3 bytes. "hello가" = 8 bytes. Limit 7 splits mid-char → back off to 5.
        let mut s = "hello가".to_string();
        truncate_at_char_boundary(&mut s, 7);
        assert_eq!(s, "hello");
    }

    #[test]
    fn truncate_multibyte_keeps_full_char_when_fits() {
        // "AB가" = 5 bytes. Limit 5 = exactly fits.
        let mut s = "AB가".to_string();
        truncate_at_char_boundary(&mut s, 5);
        assert_eq!(s, "AB가");
    }

    #[test]
    fn truncate_limit_zero_clears_string() {
        let mut s = "hello".to_string();
        truncate_at_char_boundary(&mut s, 0);
        assert_eq!(s, "");
    }

}
