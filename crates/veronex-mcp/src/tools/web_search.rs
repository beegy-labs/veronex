//! `web_search` tool — web search via SearXNG (self-hosted meta search engine).
//!
//! Env: `SEARXNG_URL` (required, e.g. `https://ai-search.girok.dev`)
//!
//! Result format: JSON array of { title, url, snippet, engine } objects.

use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::warn;

use super::Tool;

const MAX_RESULTS: usize = 5;

pub struct WebSearchTool {
    http: reqwest::Client,
    base_url: String,
}

impl WebSearchTool {
    pub fn new(http: reqwest::Client) -> Self {
        let base_url = std::env::var("SEARXNG_URL")
            .expect("SEARXNG_URL environment variable is required");
        tracing::info!(url = %base_url, "web_search: using SearXNG");
        Self { http, base_url: base_url.trim_end_matches('/').to_string() }
    }

    async fn search(&self, query: &str, count: usize) -> Result<Value, String> {
        let url = format!(
            "{}/search?q={}&format=json&pageno=1",
            self.base_url,
            urlencoding::encode(query),
        );

        let resp = self.http.get(&url)
            .send()
            .await
            .map_err(|e| format!("SearXNG request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("SearXNG returned {}", resp.status()));
        }

        let body: Value = resp.json().await
            .map_err(|e| format!("SearXNG parse error: {e}"))?;

        let results = body["results"]
            .as_array()
            .map(|arr| {
                arr.iter().take(count).map(|r| json!({
                    "title":   r["title"].as_str().unwrap_or(""),
                    "url":     r["url"].as_str().unwrap_or(""),
                    "snippet": r["content"].as_str().unwrap_or(""),
                    "engine":  r["engine"].as_str().unwrap_or("")
                })).collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if results.is_empty() {
            warn!(query = %query, "SearXNG returned no results");
        }

        Ok(json!(results))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn spec(&self) -> Value {
        json!({
            "name": "web_search",
            "description": "Search the web and return a list of relevant results with titles, URLs, and snippets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results to return (1-10, default 5)",
                        "minimum": 1,
                        "maximum": 10
                    }
                },
                "required": ["query"]
            }
        })
    }

    async fn call(&self, args: &Value) -> Result<Value, String> {
        let query = args["query"].as_str().ok_or("missing required argument: query")?;
        if query.trim().is_empty() {
            return Err("query must not be empty".to_string());
        }
        let count = args["count"].as_u64().unwrap_or(MAX_RESULTS as u64).min(10) as usize;
        self.search(query, count).await
    }
}
