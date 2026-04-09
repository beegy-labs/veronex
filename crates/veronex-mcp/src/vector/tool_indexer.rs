//! `McpToolIndexer` — sync Vespa index when tools are registered or removed.
//!
//! Called from MCP server register/delete handlers and `McpToolCache` refresh.

use tracing::{info, warn};
use uuid::Uuid;

use crate::types::McpTool;
use super::vespa_client::{McpToolDoc, VespaClient};
use super::selector::EmbedClient;

// ── Tool ID ───────────────────────────────────────────────────────────────────

/// Stable document ID for a tool: `{deployment_id}:{service_id}:{server_id}:{tool_name}`.
pub fn tool_doc_id(deployment_id: &str, service_id: &str, server_id: &str, tool_name: &str) -> String {
    format!("{deployment_id}:{service_id}:{server_id}:{tool_name}")
}

// ── Indexer ───────────────────────────────────────────────────────────────────

/// Embeds and indexes MCP tools into Vespa; removes them on server deletion.
#[derive(Clone)]
pub struct McpToolIndexer {
    vespa: VespaClient,
    embed: EmbedClient,
}

impl McpToolIndexer {
    pub fn new(vespa: VespaClient, embed: EmbedClient) -> Self {
        Self { vespa, embed }
    }

    /// Embed and upsert all tools for a server into Vespa.
    ///
    /// `deployment_id` — deployment partition key (from `VESPA_DEPLOYMENT_ID`).
    /// `service_id`    — account/tenant UUID string for multi-tenant isolation.
    /// `server_id`     — MCP server UUID string.
    pub async fn index_server_tools(
        &self,
        deployment_id: &str,
        service_id: &str,
        server_id: Uuid,
        tools: &[McpTool],
    ) {
        if tools.is_empty() {
            return;
        }

        let server_id_str = server_id.to_string();

        // Batch-embed all descriptions in one HTTP call
        let descriptions: Vec<&str> = tools.iter().map(|t| t.description.as_str()).collect();
        let embeddings = match self.embed.embed_batch(&descriptions).await {
            Ok(e) => e,
            Err(err) => {
                warn!(%server_id, error = %err, "McpToolIndexer: embed_batch failed — skipping index");
                return;
            }
        };

        if embeddings.len() != tools.len() {
            warn!(%server_id, "McpToolIndexer: embedding count mismatch — skipping index");
            return;
        }

        let mut ok = 0usize;
        let mut fail = 0usize;

        for (tool, embedding) in tools.iter().zip(embeddings.into_iter()) {
            let doc = McpToolDoc {
                tool_id:       tool_doc_id(deployment_id, service_id, &server_id_str, &tool.name),
                deployment_id: deployment_id.to_owned(),
                service_id:    service_id.to_owned(),
                server_id:    server_id_str.clone(),
                server_name:  tool.server_name.clone(),
                tool_name:    tool.name.clone(),
                description:  tool.description.clone(),
                input_schema: tool.input_schema.to_string(),
                embedding,
            };

            match self.vespa.feed(&doc).await {
                Ok(_) => ok += 1,
                Err(e) => {
                    warn!(%server_id, tool = %tool.name, error = %e, "McpToolIndexer: feed failed");
                    fail += 1;
                }
            }
        }

        info!(%server_id, ok, fail, "McpToolIndexer: index_server_tools complete");
    }

    /// Remove all Vespa documents for a server.
    pub async fn remove_server_tools(&self, deployment_id: &str, service_id: &str, server_id: Uuid) {
        let server_id_str = server_id.to_string();
        match self.vespa.delete_server(deployment_id, service_id, &server_id_str).await {
            Ok(_) => info!(%server_id, "McpToolIndexer: server tools removed from Vespa"),
            Err(e) => warn!(%server_id, error = %e, "McpToolIndexer: delete_server failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_doc_id_format() {
        let id = tool_doc_id("test-deploy", "svc-abc", "srv-xyz", "web_search");
        assert_eq!(id, "test-deploy:svc-abc:srv-xyz:web_search");
    }

    #[test]
    fn tool_doc_id_with_uuid_strings() {
        let dep = "test-deploy";
        let svc = "00000000-0000-0000-0000-000000000001";
        let srv = "00000000-0000-0000-0000-000000000002";
        let id = tool_doc_id(dep, svc, srv, "get_weather");
        assert!(id.starts_with(dep));
        assert!(id.ends_with("get_weather"));
        assert_eq!(id.matches(':').count(), 3);
    }
}
