//! Tool registry — each MCP tool implements this trait.
//!
//! To add a new tool:
//! 1. Create `tools/{name}.rs` implementing `Tool`
//! 2. Add `pub mod {name};` here
//! 3. Register in `bin/veronex-mcp.rs` main()

pub mod weather;
pub mod web_search;

use async_trait::async_trait;
use serde_json::Value;

/// Contract every MCP tool must satisfy.
///
/// `spec()` is returned as-is in `tools/list`.
/// `call()` receives the `arguments` object from `tools/call`.
#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> Value;
    async fn call(&self, args: &Value) -> Result<Value, String>;
}
