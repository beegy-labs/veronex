//! Vector selection for MCP tools.
//!
//! Flow:
//!   query → EmbedClient → Vespa ANN → Top-K McpTool definitions
//!
//! Components:
//!   - `VespaClient`        — Vespa document feed + ANN search
//!   - `McpToolIndexer`     — embed + upsert tools on server register/update
//!   - `McpVectorSelector`  — per-request query embedding + top-k selection
//!   - `EmbedClient`        — shared HTTP wrapper for veronex-embed

pub mod vespa_client;
pub mod tool_indexer;
pub mod selector;

#[cfg(test)]
mod tests;

pub use vespa_client::VespaClient;
pub use tool_indexer::McpToolIndexer;
pub use selector::{EmbedClient, McpVectorSelector};
