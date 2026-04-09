//! Vespa HTTP client — feed, delete, and ANN search for `mcp_tools` schema.
//!
//! Document API:  POST /document/v1/mcp_tools/mcp_tools/docid/{id}   (create/replace)
//! Delete sel.:   DELETE /document/v1/mcp_tools/mcp_tools/docid/?selection=...
//! Query API:     POST /search/

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// HTTP client timeout for all Vespa API requests.
const VESPA_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Reject IDs that could break YQL string interpolation.
/// Allows alphanumeric, underscores, hyphens, and colons (for tool_id composite keys).
fn validate_vespa_id(s: &str) -> Result<()> {
    if s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ':') {
        Ok(())
    } else {
        bail!("invalid Vespa ID (disallowed characters): {:?}", s)
    }
}

// ── Document model ─────────────────────────────────────────────────────────────

/// One document in the `mcp_tools` Vespa schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDoc {
    pub tool_id: String,
    pub deployment_id: String,
    pub service_id: String,
    /// MCP server UUID (used for deletion and multi-tenant filtering).
    pub server_id: String,
    /// MCP server slug — matches `McpTool::server_name` for namespaced tool lookup.
    pub server_name: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: String,
    pub embedding: Vec<f32>,
}

impl McpToolDoc {
    /// Vespa document ID — stable across upserts.
    pub fn doc_id(&self) -> &str {
        &self.tool_id
    }
}

// ── Search result ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct VespaHit {
    pub tool_id: String,
    pub deployment_id: String,
    pub service_id: String,
    pub server_id: String,
    /// Server slug — used by `hits_to_openai` to build the namespaced function name.
    pub server_name: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: String,
    pub relevance: f32,
}

// ── Vespa client ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct VespaClient {
    base_url: String,
    client: reqwest::Client,
}

impl VespaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            client: reqwest::Client::builder()
                .timeout(VESPA_HTTP_TIMEOUT)
                .build()
                .expect("VespaClient reqwest build"),
        }
    }

    // ── Feed (upsert) ──────────────────────────────────────────────────────────

    /// Upsert one tool document. Idempotent.
    pub async fn feed(&self, doc: &McpToolDoc) -> Result<()> {
        let url = format!(
            "{}/document/v1/mcp_tools/mcp_tools/docid/{}",
            self.base_url,
            urlencoding::encode(doc.doc_id())
        );

        // Vespa document API expects {"fields": {...}}
        let body = serde_json::json!({
            "fields": {
                "tool_id":       doc.tool_id,
                "deployment_id": doc.deployment_id,
                "service_id":    doc.service_id,
                "server_id":    doc.server_id,
                "server_name":  doc.server_name,
                "tool_name":    doc.tool_name,
                "description":  doc.description,
                "input_schema": doc.input_schema,
                "embedding":    { "values": doc.embedding },
            }
        });

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Vespa feed HTTP")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("Vespa feed {status}: {text}");
        }
        Ok(())
    }

    // ── Delete by selection ────────────────────────────────────────────────────

    /// Delete all documents matching `deployment_id = X AND service_id = Y AND server_id = Z`.
    pub async fn delete_server(&self, deployment_id: &str, service_id: &str, server_id: &str) -> Result<()> {
        validate_vespa_id(deployment_id)?;
        validate_vespa_id(service_id)?;
        validate_vespa_id(server_id)?;
        let selection = format!(
            "mcp_tools.deployment_id == \"{}\" and mcp_tools.service_id == \"{}\" and mcp_tools.server_id == \"{}\"",
            deployment_id, service_id, server_id
        );
        let url = format!(
            "{}/document/v1/mcp_tools/mcp_tools/docid/?selection={}&continuation=",
            self.base_url,
            urlencoding::encode(&selection)
        );

        // Vespa delete-by-selection may be paginated; loop until done.
        let mut next = Some(url);
        while let Some(ref u) = next.clone() {
            let resp = self.client
                .delete(u)
                .send()
                .await
                .context("Vespa delete HTTP")?;

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                bail!("Vespa delete {status}: {text}");
            }

            // Check for continuation token (paginated deletion)
            let json: serde_json::Value = resp.json().await.unwrap_or_default();
            next = json["continuation"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|c| format!(
                    "{}/document/v1/mcp_tools/mcp_tools/docid/?selection={}&continuation={}",
                    self.base_url,
                    urlencoding::encode(&selection),
                    urlencoding::encode(c)
                ));
        }
        Ok(())
    }

    // ── ANN search ─────────────────────────────────────────────────────────────

    /// Nearest-neighbour search filtered by `deployment_id` + `service_id`.
    /// Returns up to `top_k` hits ordered by cosine similarity (angular distance).
    pub async fn search(
        &self,
        embedding: &[f32],
        deployment_id: &str,
        service_id: &str,
        top_k: usize,
    ) -> Result<Vec<VespaHit>> {
        validate_vespa_id(deployment_id)?;
        validate_vespa_id(service_id)?;
        let url = format!("{}/search/", self.base_url);

        // YQL: filter on deployment_id + service_id + HNSW ANN.
        // Uses attribute equality (=) — correct for non-indexed string fields.
        let yql = format!(
            "select tool_id, deployment_id, service_id, server_id, server_name, tool_name, description, input_schema \
             from mcp_tools \
             where deployment_id = \"{}\" \
             and service_id = \"{}\" \
             and ({{targetHits: {top_k}}}nearestNeighbor(embedding, qe)) \
             order by closeness(field, embedding) desc \
             limit {top_k}",
            deployment_id, service_id
        );

        let body = serde_json::json!({
            "yql": yql,
            "hits": top_k,
            "ranking": "semantic",
            "input.query(qe)": { "values": embedding },
        });

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Vespa search HTTP")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("Vespa search {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await.context("Vespa search parse")?;
        let hits = parse_hits(&json);
        Ok(hits)
    }

    /// Health check — returns `true` if the Vespa query API responds.
    pub async fn is_healthy(&self) -> bool {
        let url = format!("{}/ApplicationStatus", self.base_url.replace(":8080", ":19071"));
        self.client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false)
    }
}

// ── Response parser ────────────────────────────────────────────────────────────

fn parse_hits(json: &serde_json::Value) -> Vec<VespaHit> {
    let children = json
        .pointer("/root/children")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    children
        .iter()
        .filter_map(|hit| {
            let fields = hit.get("fields")?;
            let relevance = hit["relevance"].as_f64().unwrap_or(0.0) as f32;
            Some(VespaHit {
                tool_id:       fields["tool_id"].as_str()?.to_owned(),
                deployment_id: fields["deployment_id"].as_str().unwrap_or("").to_owned(),
                service_id:    fields["service_id"].as_str()?.to_owned(),
                server_id:    fields["server_id"].as_str()?.to_owned(),
                server_name:  fields["server_name"].as_str().unwrap_or("").to_owned(),
                tool_name:    fields["tool_name"].as_str()?.to_owned(),
                description:  fields["description"].as_str().unwrap_or("").to_owned(),
                input_schema: fields["input_schema"].as_str().unwrap_or("{}").to_owned(),
                relevance,
            })
        })
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_response() {
        let json = serde_json::json!({ "root": { "children": [] } });
        assert!(parse_hits(&json).is_empty());
    }

    #[test]
    fn parse_single_hit() {
        let json = serde_json::json!({
            "root": {
                "children": [{
                    "relevance": 0.92,
                    "fields": {
                        "tool_id": "test-deploy:svc:srv:get_weather",
                        "deployment_id": "test-deploy",
                        "service_id": "svc",
                        "server_id": "srv",
                        "server_name": "weather_mcp",
                        "tool_name": "get_weather",
                        "description": "Get weather",
                        "input_schema": "{}"
                    }
                }]
            }
        });
        let hits = parse_hits(&json);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool_name, "get_weather");
        assert_eq!(hits[0].deployment_id, "test-deploy");
        assert!((hits[0].relevance - 0.92).abs() < 1e-4);
    }

    #[test]
    fn doc_id_matches_tool_id() {
        let doc = McpToolDoc {
            tool_id: "test-deploy:svc:srv:tool".into(),
            deployment_id: "test-deploy".into(),
            service_id: "svc".into(),
            server_id: "srv".into(),
            server_name: "my_server".into(),
            tool_name: "tool".into(),
            description: "desc".into(),
            input_schema: "{}".into(),
            embedding: vec![0.0; 1024],
        };
        assert_eq!(doc.doc_id(), "test-deploy:svc:srv:tool");
    }
}
