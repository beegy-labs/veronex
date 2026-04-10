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

    /// Select top-k tools relevant to `query` within `environment` + `tenant_id`.
    ///
    /// Returns `None` on any error (caller falls back to `get_all()`).
    pub async fn select(
        &self,
        query: &str,
        environment: &str,
        tenant_id: &str,
        top_k_override: Option<usize>,
    ) -> Option<Vec<VespaHit>> {
        let top_k = top_k_override.unwrap_or(self.top_k);

        // 1. Get query embedding (Valkey-cached)
        let embedding = self.get_embedding_cached(query).await
            .map_err(|e| warn!(error = %e, "McpVectorSelector: embed failed"))
            .ok()?;

        // 2. ANN search in Vespa filtered by environment + tenant_id
        let hits = self.vespa.search(&embedding, environment, tenant_id, top_k).await
            .map_err(|e| warn!(error = %e, "McpVectorSelector: search failed"))
            .ok()?;

        debug!(query_len = query.len(), hits = hits.len(), environment, tenant_id, "McpVectorSelector: selected tools");
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

    // (embed_cache_key_deterministic removed: SHA256 determinism is a library guarantee;
    //  format tested by embed_cache_key_prefix_and_hash_length)
    // (embed_cache_key_collision_free removed: SHA256 collision resistance is a library guarantee)

    fn make_hit(server_name: &str, tool_name: &str, schema: &str) -> VespaHit {
        VespaHit {
            tool_id: format!("default:svc:{server_name}:{tool_name}"),
            environment: "default".into(),
            tenant_id: "svc".into(),
            server_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            server_name: server_name.into(),
            tool_name: tool_name.into(),
            description: "desc".into(),
            input_schema: schema.into(),
            relevance: 0.9,
        }
    }

    #[test]
    fn hits_to_openai_format() {
        let hits = vec![make_hit(
            "weather_mcp", "get_weather",
            r#"{"type":"object","properties":{"city":{"type":"string"}}}"#,
        )];

        let fns = McpVectorSelector::hits_to_openai(&hits);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0]["function"]["name"], "mcp_weather_mcp_get_weather");
        assert_eq!(fns[0]["type"], "function");
    }

    // (hits_to_openai_empty_slice removed: trivial empty iterator, no logic under test)

    #[test]
    fn hits_to_openai_malformed_schema_falls_back_to_empty_object() {
        let hits = vec![make_hit("srv", "tool", "NOT_JSON!!!")];
        let fns = McpVectorSelector::hits_to_openai(&hits);
        assert_eq!(fns.len(), 1);
        // fallback: {"type":"object","properties":{}}
        assert_eq!(fns[0]["function"]["parameters"]["type"], "object");
    }

    // (hits_to_openai_preserves_order removed: stdlib iterator order is a language guarantee)

    #[test]
    fn embed_cache_key_prefix_and_hash_length() {
        let key = embed_cache_key("test query");
        let hash_part = key.strip_prefix("veronex:mcp:embed:").unwrap();
        assert_eq!(hash_part.len(), 16);
        assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn embed_client_strips_trailing_slash() {
        let c = EmbedClient::new("http://localhost:8080/");
        assert!(!c.base_url.ends_with('/'));
        let c2 = EmbedClient::new("http://localhost:8080");
        assert_eq!(c.base_url, c2.base_url);
    }
}
