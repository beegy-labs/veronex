//! MCP tool discovery + embedding pipeline.
//!
//! On each scrape cycle (or on-demand):
//!   1. Call tools/list on each online MCP server
//!   2. Compare SHA-256 hash with previous tools/list result
//!   3. If changed: embed new/modified tools via veronex-embed, delete removed tools
//!   4. Store vectors + specs in Valkey, update hash

use std::collections::{HashMap, HashSet};

use fred::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

const TOOLS_LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const EMBED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ToolsListResponse {
    result: Option<ToolsListResult>,
}

#[derive(Debug, Deserialize)]
struct ToolsListResult {
    tools: Option<Vec<ToolDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolDef {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    vector: Vec<f32>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Discover tools from all online MCP servers, embed changes, store in Valkey.
pub async fn discover_and_embed(
    http: &reqwest::Client,
    valkey: &fred::clients::Pool,
    mcp_targets: &[(String, String)], // (server_id, base_url)
    embed_url: &str,
) {
    for (server_id, base_url) in mcp_targets {
        if let Err(e) = process_server(http, valkey, server_id, base_url, embed_url).await {
            debug!(server_id, error = %e, "MCP discover+embed failed");
        }
    }
}

async fn process_server(
    http: &reqwest::Client,
    valkey: &fred::clients::Pool,
    server_id: &str,
    base_url: &str,
    embed_url: &str,
) -> anyhow::Result<()> {
    // 1. Call tools/list
    let tools = fetch_tools_list(http, base_url).await?;
    if tools.is_empty() {
        return Ok(());
    }

    // 2. Hash current tools/list for diff detection
    let current_hash = hash_tools(&tools);
    let hash_key = format!("mcp:tools_hash:{server_id}");
    let conn: fred::clients::Client = valkey.next().clone();
    let prev_hash: Option<String> = conn.get(&hash_key).await.unwrap_or(None);

    if prev_hash.as_deref() == Some(&current_hash) {
        debug!(server_id, "MCP tools unchanged — skip embed");
        return Ok(());
    }

    info!(server_id, tools = tools.len(), "MCP tools changed — re-embedding");

    // 3. Find previous tool names (for deletion)
    let prev_tool_names = get_stored_tool_names(valkey, server_id).await;
    let current_tool_names: HashSet<String> = tools.iter().map(|t| t.name.clone()).collect();

    // 4. Delete removed tools
    for removed in prev_tool_names.difference(&current_tool_names) {
        let vec_key = format!("mcp:vec:{server_id}:{removed}");
        let _: () = conn.del(&vec_key).await.unwrap_or(());
        debug!(server_id, tool = %removed, "deleted removed tool vector");
    }

    // 5. Embed + store each tool
    let texts: Vec<String> = tools
        .iter()
        .map(|t| {
            format!(
                "{}: {}",
                t.name,
                t.description.as_deref().unwrap_or("")
            )
        })
        .collect();

    let vectors = embed_batch(http, embed_url, &texts).await?;

    for (tool, vector) in tools.iter().zip(vectors.iter()) {
        let vec_key = format!("mcp:vec:{server_id}:{}", tool.name);
        let vector_bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        let spec = to_openai_spec(tool, server_id);

        let _: () = conn
            .hset(
                &vec_key,
                [
                    ("vector", serde_json::to_string(&vector_bytes).unwrap_or_default()),
                    ("text", format!("{}: {}", tool.name, tool.description.as_deref().unwrap_or(""))),
                    ("spec", serde_json::to_string(&spec).unwrap_or_default()),
                ],
            )
            .await
            .unwrap_or(());
    }

    // 6. Store tool names set (for future diff) + update hash
    let names_key = format!("mcp:tool_names:{server_id}");
    let names_json = serde_json::to_string(&current_tool_names).unwrap_or_default();
    let _: () = conn.set(&names_key, &names_json, None, None, false).await.unwrap_or(());
    let _: () = conn.set(&hash_key, &current_hash, None, None, false).await.unwrap_or(());

    info!(server_id, tools = tools.len(), "MCP tool vectors stored");
    Ok(())
}

// ── tools/list ───────────────────────────────────────────────────────────────

async fn fetch_tools_list(http: &reqwest::Client, base_url: &str) -> anyhow::Result<Vec<ToolDef>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list"
    });

    let resp = http
        .post(base_url)
        .timeout(TOOLS_LIST_TIMEOUT)
        .json(&body)
        .send()
        .await?;

    let parsed: ToolsListResponse = resp.json().await?;
    Ok(parsed
        .result
        .and_then(|r| r.tools)
        .unwrap_or_default())
}

// ── Embedding ────────────────────────────────────────────────────────────────

async fn embed_batch(
    http: &reqwest::Client,
    embed_url: &str,
    texts: &[String],
) -> anyhow::Result<Vec<Vec<f32>>> {
    let url = format!("{}/embed/batch", embed_url.trim_end_matches('/'));
    let body = serde_json::json!({ "texts": texts });

    let resp = http
        .post(&url)
        .timeout(EMBED_TIMEOUT)
        .json(&body)
        .send()
        .await?;

    #[derive(Deserialize)]
    struct BatchResp {
        vectors: Vec<Vec<f32>>,
    }

    let parsed: BatchResp = resp.json().await?;
    Ok(parsed.vectors)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn hash_tools(tools: &[ToolDef]) -> String {
    let mut sorted: Vec<String> = tools
        .iter()
        .map(|t| format!("{}:{}", t.name, t.description.as_deref().unwrap_or("")))
        .collect();
    sorted.sort();
    let joined = sorted.join("|");
    format!("{:x}", Sha256::digest(joined.as_bytes()))
}

async fn get_stored_tool_names(
    valkey: &fred::clients::Pool,
    server_id: &str,
) -> HashSet<String> {
    let conn: fred::clients::Client = valkey.next().clone();
    let names_key = format!("mcp:tool_names:{server_id}");
    let json: Option<String> = conn.get(&names_key).await.unwrap_or(None);
    json.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn to_openai_spec(tool: &ToolDef, _server_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description.as_deref().unwrap_or(""),
            "parameters": tool.input_schema
        }
    })
}
