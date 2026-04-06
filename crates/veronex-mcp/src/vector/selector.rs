//! `McpVectorSelector` — embed query + Vespa ANN → Top-K relevant MCP tools.
//!
//! Hot path: embedding is Valkey-cached (5 min TTL) to avoid redundant calls.
//! Fallback: on any error, returns `None` so the caller can use `get_all()`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use fred::prelude::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};
use super::vespa_client::{VespaClient, VespaHit};

// ── Config ─────────────────────────────────────────────────────────────────────

const EMBED_TIMEOUT: Duration = Duration::from_secs(30);
const EMBED_CACHE_TTL_SECS: i64 = 300; // 5 min

// ── EmbedClient ───────────────────────────────────────────────────────────────

/// Thin HTTP wrapper around veronex-embed `/embed` + `/embed/batch`.
#[derive(Clone)]
pub struct EmbedClient {
    base_url: String,
    client: reqwest::Client,
}

impl EmbedClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            client: reqwest::Client::builder()
                .timeout(EMBED_TIMEOUT)
                .build()
                .expect("EmbedClient reqwest build"),
        }
    }

    /// Embed a single text, returns 1024-dim vector.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        #[derive(Deserialize)]
        struct Resp { vector: Vec<f32> }

        let resp: Resp = self.client
            .post(format!("{}/embed", self.base_url))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .context("embed HTTP")?
            .error_for_status()
            .context("embed status")?
            .json()
            .await
            .context("embed parse")?;

        Ok(resp.vector)
    }

    /// Batch-embed multiple texts. Returns vectors in the same order.
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Resp { vectors: Vec<Vec<f32>> }

        let resp: Resp = self.client
            .post(format!("{}/embed/batch", self.base_url))
            .json(&serde_json::json!({ "texts": texts }))
            .send()
            .await
            .context("embed_batch HTTP")?
            .error_for_status()
            .context("embed_batch status")?
            .json()
            .await
            .context("embed_batch parse")?;

        Ok(resp.vectors)
    }
}

// ── VectorSelector ────────────────────────────────────────────────────────────

/// Selects Top-K relevant tools for a query using embedding + Vespa ANN.
#[derive(Clone)]
pub struct McpVectorSelector {
    vespa: VespaClient,
    embed: EmbedClient,
    valkey: Arc<Pool>,
    top_k: usize,
}

impl McpVectorSelector {
    pub fn new(
        vespa: VespaClient,
        embed: EmbedClient,
        valkey: Arc<Pool>,
        top_k: usize,
    ) -> Self {
        Self { vespa, embed, valkey, top_k }
    }

    /// Select top-k tools relevant to `query` within `service_id`.
    ///
    /// Returns `None` on any error (caller falls back to `get_all()`).
    pub async fn select(
        &self,
        query: &str,
        service_id: &str,
        top_k_override: Option<usize>,
    ) -> Option<Vec<VespaHit>> {
        let top_k = top_k_override.unwrap_or(self.top_k);

        // 1. Get query embedding (Valkey-cached)
        let embedding = self.get_embedding_cached(query).await
            .map_err(|e| warn!(error = %e, "McpVectorSelector: embed failed"))
            .ok()?;

        // 2. ANN search in Vespa
        let hits = self.vespa.search(&embedding, service_id, top_k).await
            .map_err(|e| warn!(error = %e, "McpVectorSelector: search failed"))
            .ok()?;

        debug!(query_len = query.len(), hits = hits.len(), service_id, "McpVectorSelector: selected tools");
        Some(hits)
    }

    /// Convert Vespa hits into OpenAI function definitions for LLM injection.
    pub fn hits_to_openai(hits: &[VespaHit]) -> Vec<serde_json::Value> {
        hits.iter().map(|h| {
            let params: serde_json::Value = serde_json::from_str(&h.input_schema)
                .unwrap_or_else(|_| serde_json::json!({ "type": "object", "properties": {} }));
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": format!("mcp_{}_{}", h.server_name, h.tool_name),
                    "description": h.description,
                    "parameters": params,
                }
            })
        }).collect()
    }

    // ── Embedding cache ────────────────────────────────────────────────────────

    async fn get_embedding_cached(&self, text: &str) -> Result<Vec<f32>> {
        let key = embed_cache_key(text);
        let conn: fred::clients::Client = self.valkey.next().clone();

        // Cache hit
        if let Ok(Some(json)) = conn.get::<Option<String>, _>(&key).await {
            if let Ok(v) = serde_json::from_str::<Vec<f32>>(&json) {
                debug!("McpVectorSelector: embed cache hit");
                return Ok(v);
            }
        }

        // Cache miss — call embed service
        let vector = self.embed.embed(text).await?;

        // Write to Valkey (non-fatal)
        if let Ok(json) = serde_json::to_string(&vector) {
            if let Err(e) = conn
                .set::<(), _, _>(&key, json, Some(Expiration::EX(EMBED_CACHE_TTL_SECS)), None, false)
                .await
            {
                warn!(error = %e, key = %key, "MCP: embed cache SET failed");
            }
        }

        Ok(vector)
    }
}

fn embed_cache_key(text: &str) -> String {
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    format!("veronex:mcp:embed:{}", &hash[..16])
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_cache_key_deterministic() {
        let k1 = embed_cache_key("hello");
        let k2 = embed_cache_key("hello");
        assert_eq!(k1, k2);
        assert!(k1.starts_with("veronex:mcp:embed:"));
    }

    #[test]
    fn embed_cache_key_collision_free() {
        let k1 = embed_cache_key("hello");
        let k2 = embed_cache_key("world");
        assert_ne!(k1, k2);
    }

    #[test]
    fn hits_to_openai_format() {
        let hits = vec![VespaHit {
            tool_id: "svc:srv:get_weather".into(),
            service_id: "svc".into(),
            server_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            server_name: "weather_mcp".into(),
            tool_name: "get_weather".into(),
            description: "Get weather for a city".into(),
            input_schema: r#"{"type":"object","properties":{"city":{"type":"string"}}}"#.into(),
            relevance: 0.95,
        }];

        let fns = McpVectorSelector::hits_to_openai(&hits);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0]["function"]["name"], "mcp_weather_mcp_get_weather");
        assert_eq!(fns[0]["type"], "function");
    }
}
