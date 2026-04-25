//! `analyze_image` tool — vision analysis via a Veronex-hosted vision model.
//!
//! Accepts a base64-encoded image and returns a text description.
//! Calls `/api/generate` on the Veronex API (not Ollama directly) so requests
//! go through the scheduler, AIMD, and routing layers.
//!
//! Env:
//!   `VERONEX_URL`          — Veronex base URL (default: `http://veronex:3000`)
//!   `VERONEX_API_KEY`      — API key for auth (required; tool returns error if unset)
//!   `ANALYZE_IMAGE_MODEL`  — vision model name registered on a Veronex provider
//!                            (default: `qwen3-vl:8b`)

use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::{debug, warn};

use super::Tool;

const DEFAULT_MODEL: &str = "qwen3-vl:8b";
const DEFAULT_PROMPT: &str = "Describe this image in detail.";
const TIMEOUT_SECS: u64 = 120;

pub struct AnalyzeImageTool {
    http: reqwest::Client,
    veronex_url: String,
    api_key: Option<String>,
    model: String,
}

impl AnalyzeImageTool {
    pub fn new(http: reqwest::Client) -> Self {
        let veronex_url = std::env::var("VERONEX_URL")
            .unwrap_or_else(|_| "http://veronex:3000".to_string());
        let veronex_url = veronex_url.trim_end_matches('/').to_string();
        let api_key = std::env::var("VERONEX_API_KEY").ok()
            .filter(|k| !k.is_empty());
        let model = std::env::var("ANALYZE_IMAGE_MODEL")
            .ok()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        tracing::info!(
            url = %veronex_url,
            model = %model,
            auth = api_key.is_some(),
            "analyze_image: ready"
        );
        Self { http, veronex_url, api_key, model }
    }

    async fn analyze(&self, image_b64: &str, prompt: &str) -> Result<String, String> {
        let api_key = self.api_key.as_deref()
            .ok_or("VERONEX_API_KEY is not configured")?;

        let url = format!("{}/api/generate", self.veronex_url);

        let body = json!({
            "model":   self.model,
            "prompt":  prompt,
            "images":  [image_b64],
            "stream":  false,
            "options": { "temperature": 0.0 }
        });

        debug!(model = %self.model, prompt = %prompt, "analyze_image: calling Veronex gateway");

        let resp = self.http
            .post(&url)
            .header("X-API-Key", api_key)
            .json(&body)
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format!("Veronex request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Veronex returned {status}: {text}"));
        }

        let json: Value = resp.json().await
            .map_err(|e| format!("Response parse error: {e}"))?;

        let text = json["response"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        if text.is_empty() {
            warn!(model = %self.model, "analyze_image: empty response");
            return Err("Empty response from vision model".to_string());
        }

        debug!(chars = text.len(), "analyze_image: done");
        Ok(text)
    }
}

#[async_trait]
impl Tool for AnalyzeImageTool {
    fn spec(&self) -> Value {
        json!({
            "name": "analyze_image",
            "description": "Analyze a base64-encoded image using a vision model and return a detailed text description. \
                            Use this tool when you receive an image that you cannot process directly — \
                            pass the returned description as context for your response.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image": {
                        "type": "string",
                        "description": "Base64-encoded image data (raw base64, no data URL prefix)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "What to ask about the image. Defaults to a general description request."
                    }
                },
                "required": ["image"]
            },
            "annotations": {
                "readOnlyHint":   true,
                "idempotentHint": true,
                "title":          "Analyze Image"
            }
        })
    }

    async fn call(&self, args: &Value) -> Result<Value, String> {
        let image = args["image"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Missing required argument: image")?;

        // Strip data URL prefix if present (e.g. "data:image/jpeg;base64,...")
        let image = match image.find(',') {
            Some(pos) => &image[pos + 1..],
            None => image,
        };

        let prompt = args["prompt"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_PROMPT);

        let description = self.analyze(image, prompt).await?;

        Ok(json!({
            "description": description,
            "model":       self.model,
        }))
    }
}
