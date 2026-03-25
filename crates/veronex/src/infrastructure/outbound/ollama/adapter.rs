use std::pin::Pin;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;
use serde::Deserialize;

use crate::application::ports::outbound::inference_provider::InferenceProviderPort;
use crate::domain::constants::{MAX_LINE_BUFFER, PROVIDER_REQUEST_TIMEOUT};
use crate::domain::entities::{InferenceJob, InferenceResult};
use crate::domain::enums::FinishReason;
use crate::domain::value_objects::StreamToken;

pub struct OllamaAdapter {
    base_url: String,
    client: reqwest::Client,
}

// ── Context length helper ───────────────────────────────────────────────────────

/// Derive the effective `num_ctx` to send to Ollama based on the model name.
///
/// Ollama uses `OLLAMA_CONTEXT_LENGTH` as the global default, but the per-request
/// `options.num_ctx` takes precedence and lets each model use its natural window:
///
/// - Models with "128k" / "200k" in their name get the matching context.
/// - Large models (70B+) are capped at 32K to keep KV cache manageable.
/// - Everything else defaults to 32K, which is well under the 200K global
///   env var and avoids over-allocating KV cache for small models.
fn model_effective_num_ctx(model: &str) -> u32 {
    let m = model.to_lowercase();
    if m.contains("200k")                        { return 204_800; }
    if m.contains("128k")                        { return 131_072; }
    if m.contains("1m")                          { return 131_072; } // 1M models: 128K practical limit
    if m.contains("72b") || m.contains("70b")    { return  32_768; }
    32_768 // sensible default for 7B–32B models
}

impl OllamaAdapter {
    #[allow(clippy::expect_used)]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::builder()
                .timeout(PROVIDER_REQUEST_TIMEOUT)
                .build()
                .expect("failed to build HTTP client"),
        }
    }
}

// ── /api/generate response types ───────────────────────────────────────────────

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
    /// Tool call responses from function-calling models (e.g. qwen3-coder).
    /// When the model responds with a tool call instead of text, `content` is None
    /// and the call details are here.  We serialise them as JSON so they are stored
    /// in result_text and visible in the dashboard instead of being silently dropped.
    #[serde(default)]
    tool_calls: Option<serde_json::Value>,
}

#[async_trait]
impl InferenceProviderPort for OllamaAdapter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let start = Instant::now();

        let url = format!("{}/api/generate", self.base_url);
        let num_ctx = model_effective_num_ctx(job.model_name.as_str());

        let mut options = serde_json::json!({ "num_ctx": num_ctx });
        if let Some(s) = job.seed { options["seed"] = serde_json::json!(s); }
        if let Some(fp) = job.frequency_penalty { options["frequency_penalty"] = serde_json::json!(fp); }
        if let Some(pp) = job.presence_penalty { options["presence_penalty"] = serde_json::json!(pp); }
        if let Some(ref st) = job.stop { options["stop"] = st.clone(); }

        let resp: GenerateResponse = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "model":   job.model_name.as_str(),
                "prompt":  job.prompt.as_str(),
                "stream":  false,
                "think":   false,
                "options": options,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let latency_ms = start.elapsed().as_millis() as u32;
        let finish_reason = match resp.done_reason.as_deref() {
            Some("length") => FinishReason::Length,
            Some("load") | None => FinishReason::Stop, // "load" is a VRAM notification, treat as stop
            _ => FinishReason::Stop,
        };

        Ok(InferenceResult {
            job_id: job.id.clone(),
            prompt_tokens: resp.prompt_eval_count.unwrap_or(0),
            completion_tokens: resp.eval_count.unwrap_or(0),
            cached_tokens: None, // Ollama does not expose KV cache hit counts
            latency_ms,
            ttft_ms: None,
            tokens: vec![resp.response],
            finish_reason,
        })
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        // Use /api/chat when the request has multi-turn messages (e.g. Ollama chat, Gemini compat).
        // Fall back to /api/generate for single-prompt requests.
        if let Some(messages) = &job.messages {
            return self.stream_chat(
                job.model_name.as_str(), messages.clone(), job.tools.clone(),
                job.images.clone(),
                job.stop.clone(), job.seed, job.response_format.clone(),
                job.frequency_penalty, job.presence_penalty,
            );
        }
        self.stream_generate(
            job.model_name.as_str(), job.prompt.as_str(), job.images.clone(),
            job.stop.clone(), job.seed, job.frequency_penalty, job.presence_penalty,
        )
    }
}

impl OllamaAdapter {
    /// Stream from Ollama `/api/generate` (single prompt).
    fn stream_generate(
        &self,
        model: &str,
        prompt: &str,
        images: Option<Vec<String>>,
        stop: Option<serde_json::Value>,
        seed: Option<u32>,
        frequency_penalty: Option<f64>,
        presence_penalty: Option<f64>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let url = format!("{}/api/generate", self.base_url);
        let client = self.client.clone();
        let model = model.to_string();
        let prompt = prompt.to_string();

        let num_ctx = model_effective_num_ctx(&model);

        Box::pin(async_stream::try_stream! {
            let mut options = serde_json::json!({ "num_ctx": num_ctx });
            if let Some(s) = seed { options["seed"] = serde_json::json!(s); }
            if let Some(fp) = frequency_penalty { options["frequency_penalty"] = serde_json::json!(fp); }
            if let Some(pp) = presence_penalty { options["presence_penalty"] = serde_json::json!(pp); }
            if let Some(ref st) = stop { options["stop"] = st.clone(); }

            let mut body = serde_json::json!({
                "model":   model,
                "prompt":  prompt,
                "stream":  true,
                "think":   false,
                "options": options,
            });
            if let Some(imgs) = images {
                body["images"] = serde_json::json!(imgs);
            }

            let response = client
                .post(&url)
                .json(&body)
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
                if buf.len() + bytes.len() > MAX_LINE_BUFFER {
                    Err(anyhow::anyhow!("response line exceeds 1 MB limit"))?;
                }
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Consume complete newline-delimited JSON lines
                while let Some(nl) = buf.find('\n') {
                    // Drain the line in-place to avoid a full-buffer re-allocation on every iteration.
                    let line: String = buf.drain(..nl).collect();
                    buf.remove(0); // consume the '\n'
                    let line = line.trim();

                    if line.is_empty() {
                        continue;
                    }

                    let chunk: GenerateResponse = serde_json::from_str(line)
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
                        tool_calls: None,
                        finish_reason: if chunk.done {
                            chunk.done_reason.clone().filter(|r| r != "load")
                        } else {
                            None
                        },
                    };

                    if chunk.done {
                        return;
                    }
                }
            }
        })
    }

    /// Stream from Ollama `/api/chat` (multi-turn messages).
    ///
    /// Forwards `tools` to Ollama so function-calling models (e.g. qwen3-coder)
    /// receive the tool definitions and can produce proper `tool_calls` responses.
    ///
    /// When the model generates a tool call instead of text content, a `StreamToken`
    /// with `tool_calls` populated (and empty `value`) is yielded, followed by the
    /// normal final token with usage counts.  Callers (HTTP handlers) must check
    /// `token.tool_calls` and format the response accordingly.
    fn stream_chat(
        &self,
        model: &str,
        messages: serde_json::Value,
        tools: Option<serde_json::Value>,
        images: Option<Vec<String>>,
        stop: Option<serde_json::Value>,
        seed: Option<u32>,
        response_format: Option<serde_json::Value>,
        frequency_penalty: Option<f64>,
        presence_penalty: Option<f64>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let url = format!("{}/api/chat", self.base_url);
        let client = self.client.clone();
        let num_ctx = model_effective_num_ctx(model);
        let model = model.to_string();

        Box::pin(async_stream::try_stream! {
            let mut options = serde_json::json!({ "num_ctx": num_ctx });
            if let Some(s) = seed { options["seed"] = serde_json::json!(s); }
            if let Some(fp) = frequency_penalty { options["frequency_penalty"] = serde_json::json!(fp); }
            if let Some(pp) = presence_penalty { options["presence_penalty"] = serde_json::json!(pp); }
            if let Some(ref st) = stop { options["stop"] = st.clone(); }

            // ── Normalize messages for Ollama's /api/chat format ─────────────
            // Ollama /api/chat differs from OpenAI format in two ways:
            //   1. assistant tool_calls: `arguments` must be a JSON object, not string.
            //   2. tool result messages: must not contain `tool_call_id` or `name`.
            let messages = {
                let mut msgs = messages;
                if let Some(arr) = msgs.as_array_mut() {
                    for msg in arr.iter_mut() {
                        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                        if role == "assistant" {
                            // Parse arguments strings back to objects.
                            if let Some(tcs) = msg.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                                for tc in tcs.iter_mut() {
                                    if let Some(args) = tc.pointer_mut("/function/arguments") {
                                        if let Some(s) = args.as_str() {
                                            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(s) {
                                                *args = obj;
                                            }
                                        }
                                    }
                                }
                            }
                        } else if role == "tool" {
                            // Strip OpenAI-only fields that Ollama doesn't accept.
                            if let Some(obj) = msg.as_object_mut() {
                                obj.remove("tool_call_id");
                                obj.remove("name");
                            }
                        }
                    }
                }
                msgs
            };

            // Inject images into the last user message (Ollama expects per-message images).
            let messages = if let Some(imgs) = images {
                let mut msgs = messages;
                if let Some(arr) = msgs.as_array_mut() {
                    if let Some(last_user) = arr.iter_mut().rev()
                        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    {
                        last_user["images"] = serde_json::json!(imgs);
                    }
                }
                msgs
            } else {
                messages
            };

            let mut body = serde_json::json!({
                "model":    model,
                "messages": messages,
                "stream":   true,
                "think":    false,
                "options":  options,
            });

            // Forward tool definitions so the model can produce tool_calls responses.
            if let Some(t) = tools {
                body["tools"] = t;
            }

            // Map OpenAI response_format to Ollama format field.
            if let Some(rf) = response_format {
                if rf.get("type").and_then(|t| t.as_str()) == Some("json_object") {
                    body["format"] = serde_json::json!("json");
                } else if let Some(schema) = rf.get("json_schema").and_then(|s| s.get("schema")) {
                    body["format"] = schema.clone();
                }
            }

            let response = client
                .post(&url)
                .json(&body)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                Err(anyhow::anyhow!("Ollama /api/chat returned {status}"))?;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();
            let mut emitted_tool_calls = false;

            while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                if buf.len() + bytes.len() > MAX_LINE_BUFFER {
                    Err(anyhow::anyhow!("response line exceeds 1 MB limit"))?;
                }
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(nl) = buf.find('\n') {
                    // Drain the line in-place to avoid a full-buffer re-allocation on every iteration.
                    let line: String = buf.drain(..nl).collect();
                    buf.remove(0); // consume the '\n'
                    let line = line.trim();

                    if line.is_empty() {
                        continue;
                    }

                    let chunk: ChatChunk = serde_json::from_str(line)
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

                    // Check for tool_calls in the message.
                    // When the model calls a tool, content is empty and tool_calls is set.
                    // We emit a dedicated StreamToken carrying the tool_calls so HTTP handlers
                    // can format the response correctly (OpenAI delta vs Ollama NDJSON).
                    if let Some(ref msg) = chunk.message
                        && let Some(ref tc) = msg.tool_calls
                            && !emitted_tool_calls {
                                emitted_tool_calls = true;
                                yield StreamToken {
                                    value: String::new(),
                                    is_final: false,
                                    prompt_tokens: None,
                                    completion_tokens: None,
                                    cached_tokens: None,
                                    tool_calls: Some(tc.clone()),
                                    finish_reason: None,
                                };
                            }

                    // Text content token (normal streaming text).
                    let content = chunk.message.as_ref()
                        .and_then(|m| m.content.as_deref())
                        .filter(|c| !c.is_empty())
                        .map(str::to_string)
                        .unwrap_or_default();

                    // Always emit the final token (even if empty) so usage counts arrive.
                    if chunk.done || !content.is_empty() {
                        yield StreamToken {
                            value: content,
                            is_final: chunk.done,
                            prompt_tokens,
                            completion_tokens,
                            cached_tokens: None,
                            tool_calls: None,
                            finish_reason: if chunk.done {
                                chunk.done_reason.clone().filter(|r| r != "load")
                            } else {
                                None
                            },
                        };
                    }

                    if chunk.done {
                        return;
                    }
                }
            }
        })
    }
}
