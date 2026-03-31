//! `analyze_image` tool — vision analysis via qwen3-vl:8b on Ollama.
//!
//! Accepts a base64-encoded image and returns a text description.
//! Use this for non-vision models that receive image inputs — pass the
//! description back as context instead of the raw image.
//!
//! Env: `OLLAMA_URL` (default: `http://localhost:11434`)

use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::{debug, warn};

use super::Tool;

const MODEL: &str = "qwen3-vl:8b";
const DEFAULT_PROMPT: &str = "Describe this image in detail.";
const OLLAMA_TIMEOUT_SECS: u64 = 60;

pub struct AnalyzeImageTool {
    http: reqwest::Client,
    ollama_url: String,
}

impl AnalyzeImageTool {
    pub fn new(http: reqwest::Client) -> Self {
        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let ollama_url = ollama_url.trim_end_matches('/').to_string();
        tracing::info!(url = %ollama_url, model = MODEL, "analyze_image: ready");
        Self { http, ollama_url }
    }

    async fn analyze(&self, image_b64: &str, prompt: &str) -> Result<String, String> {
        let url = format!("{}/api/generate", self.ollama_url);

        let body = json!({
            "model":  MODEL,
            "prompt": prompt,
            "images": [image_b64],
            "stream": false,
            "options": { "temperature": 0.0 }
        });

        debug!(model = MODEL, prompt = %prompt, "analyze_image: calling Ollama");

        let resp = self.http
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(OLLAMA_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama returned {status}: {text}"));
        }

        let json: Value = resp.json().await
            .map_err(|e| format!("Ollama response parse error: {e}"))?;

        let text = json["response"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        if text.is_empty() {
            warn!(model = MODEL, "analyze_image: empty response from Ollama");
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
        let image = if let Some(pos) = image.find(",") {
            &image[pos + 1..]
        } else {
            image
        };

        let prompt = args["prompt"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_PROMPT);

        let description = self.analyze(image, prompt).await?;

        Ok(json!({
            "description": description,
            "model": MODEL
        }))
    }
}
