use std::pin::Pin;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::inference_backend::InferenceBackendPort;
use crate::domain::entities::{InferenceJob, InferenceResult};
use crate::domain::enums::FinishReason;
use crate::domain::value_objects::StreamToken;

pub struct OllamaAdapter {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

// ── /api/generate response types ───────────────────────────────────────────────

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    /// Disable extended thinking (qwen3 and similar models).
    think: bool,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
    done: bool,
    /// "stop" = normal end · "load" = model just loaded into VRAM (not a real completion)
    done_reason: Option<String>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
}

// ── /api/chat response types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    message: Option<ChatChunkMessage>,
    done: bool,
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct ChatChunkMessage {
    #[serde(default)]
    content: Option<String>,
}

#[async_trait]
impl InferenceBackendPort for OllamaAdapter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let start = Instant::now();

        let url = format!("{}/api/generate", self.base_url);
        let body = GenerateRequest {
            model: job.model_name.as_str(),
            prompt: job.prompt.as_str(),
            stream: false,
            think: false,
        };

        let resp: GenerateResponse = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let latency_ms = start.elapsed().as_millis() as u32;

        Ok(InferenceResult {
            job_id: job.id.clone(),
            prompt_tokens: resp.prompt_eval_count.unwrap_or(0),
            completion_tokens: resp.eval_count.unwrap_or(0),
            cached_tokens: None, // Ollama does not expose KV cache hit counts
            latency_ms,
            ttft_ms: None,
            tokens: vec![resp.response],
            finish_reason: FinishReason::Stop,
        })
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        // Use /api/chat when the request has multi-turn messages (e.g. Ollama chat, Gemini compat).
        // Fall back to /api/generate for single-prompt requests.
        if let Some(messages) = &job.messages {
            return self.stream_chat(job.model_name.as_str(), messages.clone());
        }
        self.stream_generate(job.model_name.as_str(), job.prompt.as_str())
    }
}

impl OllamaAdapter {
    /// Stream from Ollama `/api/generate` (single prompt).
    fn stream_generate(
        &self,
        model: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let url = format!("{}/api/generate", self.base_url);
        let client = self.client.clone();
        let model = model.to_string();
        let prompt = prompt.to_string();

        Box::pin(async_stream::try_stream! {
            let response = client
                .post(&url)
                .json(&serde_json::json!({
                    "model": model,
                    "prompt": prompt,
                    "stream": true,
                    // Disable extended thinking — keeps eval_count accurate for
                    // visible output only and removes <think>…</think> from the stream.
                    "think": false,
                }))
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                Err(anyhow::anyhow!("Ollama returned {status}"))?;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Consume complete newline-delimited JSON lines
                while let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim().to_string();
                    buf = buf[nl + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    let chunk: GenerateResponse = serde_json::from_str(&line)
                        .map_err(|e| anyhow::anyhow!("failed to parse Ollama generate response: {e}: {line}"))?;

                    // Ollama sends a done_reason:"load" chunk when the model is first
                    // loaded into VRAM.  This is a notification, not a completion —
                    // skip it and continue reading the actual generation output.
                    if chunk.done && chunk.done_reason.as_deref() == Some("load") {
                        continue;
                    }

                    // On the final chunk Ollama reports token counts.
                    let (prompt_tokens, completion_tokens) = if chunk.done {
                        (chunk.prompt_eval_count, chunk.eval_count)
                    } else {
                        (None, None)
                    };

                    yield StreamToken {
                        value: chunk.response,
                        is_final: chunk.done,
                        prompt_tokens,
                        completion_tokens,
                        cached_tokens: None,
                    };

                    if chunk.done {
                        return;
                    }
                }
            }
        })
    }

    /// Stream from Ollama `/api/chat` (multi-turn messages).
    fn stream_chat(
        &self,
        model: &str,
        messages: serde_json::Value,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let url = format!("{}/api/chat", self.base_url);
        let client = self.client.clone();
        let model = model.to_string();

        Box::pin(async_stream::try_stream! {
            let response = client
                .post(&url)
                .json(&serde_json::json!({
                    "model": model,
                    "messages": messages,
                    "stream": true,
                    "think": false,
                }))
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                Err(anyhow::anyhow!("Ollama /api/chat returned {status}"))?;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim().to_string();
                    buf = buf[nl + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    let chunk: ChatChunk = serde_json::from_str(&line)
                        .map_err(|e| anyhow::anyhow!("failed to parse Ollama chat response: {e}: {line}"))?;

                    // Skip model-load notification
                    if chunk.done && chunk.done_reason.as_deref() == Some("load") {
                        continue;
                    }

                    let (prompt_tokens, completion_tokens) = if chunk.done {
                        (chunk.prompt_eval_count, chunk.eval_count)
                    } else {
                        (None, None)
                    };

                    let content = chunk
                        .message
                        .as_ref()
                        .and_then(|m| m.content.as_deref())
                        .unwrap_or("")
                        .to_string();

                    yield StreamToken {
                        value: content,
                        is_final: chunk.done,
                        prompt_tokens,
                        completion_tokens,
                        cached_tokens: None,
                    };

                    if chunk.done {
                        return;
                    }
                }
            }
        })
    }
}
