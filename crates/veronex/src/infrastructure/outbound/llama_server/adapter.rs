//! `LlamaServerAdapter` — outbound adapter against `llama-server` (llama.cpp HTTP).
//!
//! Phase 1 (this adapter): assumes the llama-server process is already running
//! (operator-managed or future Phase 3 ProcessManager). We treat `ensure_ready`
//! as a synchronous `/health` check and never trigger a load ourselves.
//!
//! - Inference path: OpenAI-compatible `POST /v1/chat/completions`
//!   - `stream=false` for `infer`
//!   - `stream=true` (SSE `data:` framing) for `stream_tokens`
//! - Lifecycle path: GET `/health` → `SlotStatus`. No in-flight coalescing
//!   needed — llama-server does not have ollama's per-runner-subprocess problem.
//! - `evict` is a no-op: in external mode we cannot unload from outside the
//!   process. Phase 3 ProcessManager owns the actual lifecycle.

use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;
use serde::Deserialize;

use crate::application::ports::outbound::inference_provider::InferenceProviderPort;
use crate::application::ports::outbound::model_lifecycle::{LifecycleOutcome, ModelLifecyclePort};
use crate::domain::constants::{MAX_LINE_BUFFER, PROVIDER_REQUEST_TIMEOUT};
use crate::domain::entities::{InferenceJob, InferenceResult};
use crate::domain::enums::FinishReason;
use crate::domain::errors::LifecycleError;
use crate::domain::value_objects::{EvictionReason, ModelInstanceState, StreamToken};
use crate::infrastructure::outbound::llama_server::health::{get_health, SlotStatus};

/// HTTP adapter for a single llama-server instance.
///
/// Holds the base URL plus a single shared `reqwest::Client` so connection
/// pooling works across requests. `provider_id` is carried for tracing and
/// future Valkey-keyed slots cache (Phase 4 prefix-aware routing).
pub struct LlamaServerAdapter {
    base_url: String,
    client: reqwest::Client,
    provider_id: uuid::Uuid,
}

impl LlamaServerAdapter {
    #[allow(clippy::expect_used)]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::builder()
                .timeout(PROVIDER_REQUEST_TIMEOUT)
                .build()
                .expect("failed to build HTTP client"),
            provider_id: uuid::Uuid::nil(),
        }
    }

    pub fn with_provider_id(mut self, id: uuid::Uuid) -> Self {
        self.provider_id = id;
        self
    }

    pub fn provider_id(&self) -> uuid::Uuid {
        self.provider_id
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Issue `/health` and surface the parsed `SlotStatus`. Used by the router
    /// (slots-aware scoring) and `ModelLifecyclePort::instance_state`.
    pub async fn health(&self) -> Result<SlotStatus> {
        get_health(&self.client, &self.base_url).await
    }
}

// ── ModelLifecyclePort ──────────────────────────────────────────────────────

#[async_trait]
impl ModelLifecyclePort for LlamaServerAdapter {
    /// Phase 1: external-mode assumption. `/health` ok ⇒ AlreadyLoaded.
    /// No probe / coalesce / num_ctx SSOT — that's all Phase 2/3 territory.
    async fn ensure_ready(&self, _model: &str) -> Result<LifecycleOutcome, LifecycleError> {
        match self.health().await {
            Ok(s) if s.is_ok() => Ok(LifecycleOutcome::AlreadyLoaded),
            Ok(s) => Err(LifecycleError::ProviderError(format!(
                "llama-server reports status={:?}",
                s.status
            ))),
            Err(e) => Err(LifecycleError::ProviderError(format!("health: {e}"))),
        }
    }

    async fn instance_state(&self, _model: &str) -> ModelInstanceState {
        match self.health().await {
            Ok(s) if s.is_ok() => ModelInstanceState::Loaded {
                loaded_at: std::time::SystemTime::now(),
                weight_bytes: 0,
            },
            _ => ModelInstanceState::NotLoaded,
        }
    }

    /// External mode no-op. Phase 3 ProcessManager.stop() is the real eviction.
    async fn evict(&self, _model: &str, _reason: EvictionReason) -> Result<(), LifecycleError> {
        Ok(())
    }
}

// ── /v1/chat/completions wire types (OpenAI-compatible) ────────────────────

#[derive(Deserialize)]
struct ChatCompletion {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: Option<String>,
    // tool_calls are surfaced via the streaming path (`stream_tokens`).
    // Non-streaming `infer` flattens the assistant turn to plain text — same
    // contract OllamaAdapter follows.
}

#[derive(Deserialize, Default)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

// ── Streaming chunk types (SSE `data: {...}`) ──────────────────────────────

#[derive(Deserialize)]
struct ChatChunk {
    choices: Vec<ChatChunkChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChunkChoice {
    delta: ChatChunkDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct ChatChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<serde_json::Value>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build the request body for /v1/chat/completions. Reused by `infer` and
/// `stream_tokens` so both wire paths send identical sampling parameters.
fn build_chat_body(job: &InferenceJob, stream: bool) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": job.model_name.as_str(),
        "stream": stream,
    });

    // Messages: prefer the explicit messages array (multi-turn / tool-calling);
    // synthesize a single user turn from `prompt` otherwise.
    if let Some(messages) = &job.messages {
        body["messages"] = messages.clone();
    } else {
        body["messages"] = serde_json::json!([
            { "role": "user", "content": job.prompt.as_str() }
        ]);
    }

    if let Some(tools) = &job.tools {
        body["tools"] = tools.clone();
    }
    if let Some(stop) = &job.stop {
        body["stop"] = stop.clone();
    }
    if let Some(seed) = job.seed {
        body["seed"] = serde_json::json!(seed);
    }
    if let Some(rf) = &job.response_format {
        body["response_format"] = rf.clone();
    }
    if let Some(fp) = job.frequency_penalty {
        body["frequency_penalty"] = serde_json::json!(fp);
    }
    if let Some(pp) = job.presence_penalty {
        body["presence_penalty"] = serde_json::json!(pp);
    }
    if let Some(mt) = job.max_tokens {
        body["max_tokens"] = serde_json::json!(mt);
    }

    body
}

/// Map an OpenAI `finish_reason` string to our domain enum.
fn map_finish_reason(s: Option<&str>) -> FinishReason {
    match s {
        Some("length") => FinishReason::Length,
        Some("stop") | Some("tool_calls") | None => FinishReason::Stop,
        _ => FinishReason::Stop,
    }
}

// ── InferenceProviderPort ──────────────────────────────────────────────────

#[async_trait]
impl InferenceProviderPort for LlamaServerAdapter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let start = Instant::now();
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let body = build_chat_body(job, false);

        let resp: ChatCompletion = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?
            .error_for_status()
            .with_context(|| "non-2xx from /v1/chat/completions")?
            .json()
            .await
            .with_context(|| "parse /v1/chat/completions JSON")?;

        let latency_ms = start.elapsed().as_millis() as u32;
        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("llama-server returned 0 choices"))?;

        let text = choice.message.content.unwrap_or_default();
        let finish_reason = map_finish_reason(choice.finish_reason.as_deref());
        let usage = resp.usage.unwrap_or_default();

        Ok(InferenceResult {
            job_id: job.id.clone(),
            prompt_tokens: usage.prompt_tokens.unwrap_or(0),
            completion_tokens: usage.completion_tokens.unwrap_or(0),
            cached_tokens: None,
            latency_ms,
            ttft_ms: None,
            tokens: vec![text],
            finish_reason,
        })
    }

    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>> {
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let client = self.client.clone();
        let body = build_chat_body(job, true);

        Box::pin(async_stream::try_stream! {
            let response = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("POST {url}"))?;

            let status = response.status();
            if !status.is_success() {
                Err(anyhow::anyhow!("llama-server returned {status}"))?;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();

            // Track usage so we can emit it on the final `[DONE]` token.
            let mut last_usage: Option<ChatUsage> = None;
            let mut last_finish_reason: Option<String> = None;

            'recv: while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                if buf.len() + bytes.len() > MAX_LINE_BUFFER {
                    Err(anyhow::anyhow!("SSE line exceeds buffer limit"))?;
                }
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // OpenAI SSE: events are separated by `\n\n`; each event has
                // one or more `field: value` lines. We only care about `data:`.
                while let Some(boundary) = buf.find("\n\n") {
                    let event: String = buf.drain(..boundary).collect();
                    buf.drain(..2); // strip the "\n\n"

                    for raw_line in event.split('\n') {
                        let line = raw_line.trim();
                        if line.is_empty() || !line.starts_with("data:") {
                            continue;
                        }
                        let payload = line.trim_start_matches("data:").trim();
                        if payload == "[DONE]" {
                            break 'recv;
                        }

                        let chunk: ChatChunk = serde_json::from_str(payload).map_err(|e| {
                            anyhow::anyhow!("parse SSE chunk: {e}: {payload}")
                        })?;

                        if let Some(u) = chunk.usage { last_usage = Some(u); }

                        for choice in chunk.choices {
                            if let Some(fr) = choice.finish_reason {
                                last_finish_reason = Some(fr);
                            }
                            let text = choice.delta.content.unwrap_or_default();
                            let tool_calls = choice.delta.tool_calls;
                            if !text.is_empty() || tool_calls.is_some() {
                                let mut tok = StreamToken::text(text);
                                tok.tool_calls = tool_calls;
                                yield tok;
                            }
                        }
                    }
                }
            }

            // Emit a final synthetic token carrying usage + finish_reason so
            // the runner can record real counts. Mirrors OllamaAdapter pattern.
            let mut done = StreamToken::done();
            if let Some(u) = last_usage {
                done.prompt_tokens = u.prompt_tokens;
                done.completion_tokens = u.completion_tokens;
            }
            done.finish_reason = last_finish_reason;
            yield done;
        })
    }
}

// `Arc<LlamaServerAdapter>` is what callers hold in the registry. The blanket
// `LlmProviderPort` impl in `inference_provider.rs` covers this — no explicit
// trait wiring needed here.
#[allow(dead_code)]
pub(crate) type ArcAdapter = Arc<LlamaServerAdapter>;

// Keep the unused import lint quiet for the `Duration` import — it's intended
// to remain available for future timeout overrides on infer/stream paths.
#[allow(dead_code)]
const _UNUSED_DURATION_TYPE: fn() -> Duration = || PROVIDER_REQUEST_TIMEOUT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::value_objects::{JobId, ModelName, Prompt};
    use chrono::Utc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn job_with_prompt(prompt: &str, model: &str) -> InferenceJob {
        InferenceJob {
            id: JobId::new(),
            prompt: Prompt::new(prompt.into()).unwrap(),
            prompt_preview: None,
            model_name: ModelName::new(model.into()).unwrap(),
            status: crate::domain::enums::JobStatus::Pending,
            provider_type: crate::domain::enums::ProviderType::LlamaServer,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
            result_text: None,
            api_key_id: None,
            account_id: None,
            latency_ms: None,
            ttft_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            source: crate::domain::enums::JobSource::Api,
            provider_id: None,
            api_format: crate::domain::enums::ApiFormat::OpenaiCompat,
            messages: None,
            tools: None,
            max_tokens: None,
            request_path: None,
            queue_time_ms: None,
            cancelled_at: None,
            conversation_id: None,
            tool_calls_json: None,
            messages_hash: None,
            messages_prefix_hash: None,
            failure_reason: None,
            images: None,
            image_keys: None,
            stop: None,
            seed: None,
            response_format: None,
            frequency_penalty: None,
            presence_penalty: None,
            mcp_loop_id: None,
            vision_analysis: None,
        }
    }

    #[tokio::test]
    async fn ensure_ready_ok_when_health_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok", "slots_idle": 4, "slots_processing": 0
            })))
            .mount(&server)
            .await;

        let adapter = LlamaServerAdapter::new(server.uri());
        let r = adapter.ensure_ready("any-model").await.unwrap();
        assert_eq!(r, LifecycleOutcome::AlreadyLoaded);
    }

    #[tokio::test]
    async fn ensure_ready_err_on_5xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let adapter = LlamaServerAdapter::new(server.uri());
        let r = adapter.ensure_ready("any-model").await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn instance_state_reflects_health() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok"
            })))
            .mount(&server)
            .await;

        let adapter = LlamaServerAdapter::new(server.uri());
        let s = adapter.instance_state("any-model").await;
        assert!(matches!(s, ModelInstanceState::Loaded { .. }));
    }

    #[tokio::test]
    async fn infer_round_trip_parses_choice_and_usage() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": { "role": "assistant", "content": "hello world" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
            })))
            .mount(&server)
            .await;

        let adapter = LlamaServerAdapter::new(server.uri());
        let job = job_with_prompt("hi", "qwen3");
        let r = adapter.infer(&job).await.unwrap();
        assert_eq!(r.tokens, vec!["hello world"]);
        assert_eq!(r.prompt_tokens, 5);
        assert_eq!(r.completion_tokens, 2);
        assert_eq!(r.finish_reason, FinishReason::Stop);
    }

    #[tokio::test]
    async fn stream_tokens_yields_deltas_and_final_usage() {
        let server = MockServer::start().await;

        // Build a small SSE response: two content deltas + DONE.
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\
                          \"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n\
                   data: [DONE]\n\n";

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;

        let adapter = LlamaServerAdapter::new(server.uri());
        let job = job_with_prompt("hi", "qwen3");
        let mut s = adapter.stream_tokens(&job);
        let mut texts = Vec::new();
        let mut final_completion_tokens = None;
        while let Some(t) = s.next().await {
            let tok = t.unwrap();
            if tok.is_final {
                final_completion_tokens = tok.completion_tokens;
            } else {
                texts.push(tok.value);
            }
        }
        assert_eq!(texts, vec!["hel", "lo"]);
        assert_eq!(final_completion_tokens, Some(2));
    }

    #[tokio::test]
    async fn evict_is_no_op() {
        let adapter = LlamaServerAdapter::new("http://127.0.0.1:1");
        let r = adapter.evict("model", EvictionReason::Operator).await;
        assert!(r.is_ok());
    }
}
