use std::pin::Pin;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};

use crate::application::ports::outbound::inference_provider::InferenceProviderPort;
use crate::application::ports::outbound::model_lifecycle::{
    LifecycleOutcome, ModelLifecyclePort,
};
use crate::domain::constants::{MAX_LINE_BUFFER, PROVIDER_REQUEST_TIMEOUT};
use crate::domain::entities::{InferenceJob, InferenceResult};
use crate::domain::enums::FinishReason;
use crate::domain::errors::LifecycleError;
use crate::domain::value_objects::{EvictionReason, ModelInstanceState, StreamToken};

pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";

pub struct GeminiAdapter {
    api_key: String,
    client: reqwest::Client,
}

impl GeminiAdapter {
    #[allow(clippy::expect_used)]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::builder()
                .timeout(PROVIDER_REQUEST_TIMEOUT)
                .build()
                .expect("failed to build HTTP client"),
        }
    }
}

// ── ModelLifecyclePort impl (no-op for cloud provider) ──────────────────────
//
// Gemini is a cloud API — no local VRAM lifecycle. All lifecycle calls
// short-circuit so the runner's Phase-1 step is uniform across providers.
// SDD: `.specs/veronex/inference-lifecycle-sod.md` §6.1 (Gemini no-op row).

#[async_trait]
impl ModelLifecyclePort for GeminiAdapter {
    async fn ensure_ready(&self, _model: &str) -> Result<LifecycleOutcome, LifecycleError> {
        Ok(LifecycleOutcome::AlreadyLoaded)
    }

    async fn instance_state(&self, _model: &str) -> ModelInstanceState {
        ModelInstanceState::Loaded {
            loaded_at: std::time::SystemTime::now(),
            weight_bytes: 0,
        }
    }

    async fn evict(&self, _model: &str, _reason: EvictionReason) -> Result<(), LifecycleError> {
        Ok(())
    }
}

// ── Gemini request / response types ───────────────────────────────────────────

#[derive(Serialize)]
struct GenerateRequest<'a> {
    contents: [Content<'a>; 1],
}

#[derive(Serialize)]
struct Content<'a> {
    parts: [Part<'a>; 1],
}

#[derive(Serialize)]
struct Part<'a> {
    text: &'a str,
}

impl<'a> GenerateRequest<'a> {
    fn new(prompt: &'a str) -> Self {
        Self {
            contents: [Content {
                parts: [Part { text: prompt }],
            }],
        }
    }
}

#[derive(Deserialize)]
struct GenerateResponse {
    candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Vec<CandidatePart>,
}

#[derive(Deserialize)]
struct CandidatePart {
    text: Option<String>,
    /// Gemini function call: `{"name": "tool_name", "args": {...}}`
    #[serde(rename = "functionCall")]
    function_call: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
    /// Tokens served from a cached context (Context Caching API).
    /// Non-zero only when a `cachedContent` is referenced in the request.
    /// Billed at ~25% of the normal input token rate.
    #[serde(rename = "cachedContentTokenCount")]
    cached_content_token_count: Option<u32>,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn extract_text(candidates: &[Candidate]) -> String {
    candidates
        .first()
        .and_then(|c| c.content.as_ref())
        .map(|c| {
            c.parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Extract `functionCall` parts from Gemini candidates, converted to Ollama tool_calls format.
/// Returns None when no function calls are present.
fn extract_function_calls(candidates: &[Candidate]) -> Option<serde_json::Value> {
    let calls: Vec<serde_json::Value> = candidates
        .first()
        .and_then(|c| c.content.as_ref())
        .map(|c| {
            c.parts
                .iter()
                .filter_map(|p| p.function_call.as_ref())
                .map(|fc| {
                    // Normalise Gemini `{name, args}` to Ollama `{function: {name, arguments}}`
                    serde_json::json!({
                        "function": {
                            "name": fc.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "arguments": fc.get("args").cloned().unwrap_or(serde_json::Value::Null),
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    if calls.is_empty() { None } else { Some(serde_json::Value::Array(calls)) }
}

fn is_done(candidates: &[Candidate]) -> bool {
    candidates
        .first()
        .and_then(|c| c.finish_reason.as_deref())
        .is_some_and(|r| !r.is_empty())
}

fn extract_usage(resp: &GenerateResponse) -> (Option<u32>, Option<u32>, Option<u32>) {
    let usage = resp.usage_metadata.as_ref();
    let prompt = usage.and_then(|u| u.prompt_token_count);
    let completion = usage.and_then(|u| u.candidates_token_count);
    let cached = usage.and_then(|u| u.cached_content_token_count).filter(|&v| v > 0);
    (prompt, completion, cached)
}

fn map_finish_reason(candidates: &[Candidate]) -> FinishReason {
    match candidates
        .first()
        .and_then(|c| c.finish_reason.as_deref())
        .unwrap_or("STOP")
    {
        "MAX_TOKENS" => FinishReason::Length,
        "CANCELLED" => FinishReason::Cancelled,
        "ERROR" | "SAFETY" | "RECITATION" | "OTHER" => FinishReason::Error,
        _ => FinishReason::Stop,
    }
}

// ── InferenceProviderPort impl ──────────────────────────────────────────────────

#[async_trait]
impl InferenceProviderPort for GeminiAdapter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let start = Instant::now();
        let model = job.model_name.as_str();
        let url = format!(
            "{GEMINI_BASE_URL}/v1beta/models/{model}:generateContent"
        );

        let body = GenerateRequest::new(job.prompt.as_str());
        let resp: GenerateResponse = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let latency_ms = start.elapsed().as_millis() as u32;
        let text = extract_text(&resp.candidates);
        let finish_reason = map_finish_reason(&resp.candidates);
        let (prompt_tokens, completion_tokens, cached_tokens) = extract_usage(&resp);

        Ok(InferenceResult {
            job_id: job.id.clone(),
            prompt_tokens: prompt_tokens.unwrap_or(0),
            completion_tokens: completion_tokens.unwrap_or(0),
            cached_tokens,
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
        let model = job.model_name.as_str().to_string();
        let prompt = job.prompt.as_str().to_string();
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        Box::pin(async_stream::try_stream! {
            if api_key.is_empty() {
                Err(anyhow::anyhow!("Gemini provider has no API key configured"))?;
            }

            let url = format!(
                "{GEMINI_BASE_URL}/v1beta/models/{model}:streamGenerateContent?alt=sse"
            );
            let body = GenerateRequest::new(&prompt);

            // error_for_status() consumes the response on error (returns Err)
            // and returns Ok(response) on success, preserving it for byte streaming.
            let response = client
                .post(&url)
                .header("x-goog-api-key", &api_key)
                .json(&body)
                .send()
                .await?
                .error_for_status()
                .map_err(|e| anyhow::anyhow!("Gemini API error: {e}"))?;

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                if buf.len() + bytes.len() > MAX_LINE_BUFFER {
                    Err(anyhow::anyhow!("response line exceeds 1 MB limit"))?;
                }
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete lines from the SSE stream.
                // Gemini SSE lines look like:  "data: {...json...}"
                while let Some(nl) = buf.find('\n') {
                    // Drain the line in-place to avoid a full-buffer re-allocation on every iteration.
                    let line: String = buf.drain(..nl).collect();
                    buf.remove(0); // consume the '\n'
                    let line = line.trim();

                    let json_str = match line.strip_prefix("data:") {
                        Some(s) => s.trim(),
                        None => continue, // skip blank / comment / event: lines
                    };

                    if json_str.is_empty() {
                        continue;
                    }

                    // Soft parse: skip unparseable lines (e.g. Gemini comment lines)
                    // rather than terminating the stream.
                    let parsed: GenerateResponse = match serde_json::from_str(json_str) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("skipping unparseable Gemini SSE line: {e}");
                            continue;
                        }
                    };

                    let text = extract_text(&parsed.candidates);
                    let tool_calls = extract_function_calls(&parsed.candidates);
                    let done = is_done(&parsed.candidates);

                    let (prompt_tokens, completion_tokens, cached_tokens) = if done {
                        extract_usage(&parsed)
                    } else {
                        (None, None, None)
                    };

                    // Populate finish_reason on the final token so HTTP handlers can
                    // propagate it correctly (e.g. gemini_compat_handlers finish_reason mapping).
                    let finish_reason = if done {
                        Some(map_finish_reason(&parsed.candidates).as_str().to_string())
                    } else {
                        None
                    };

                    // Emit a dedicated token for tool calls (empty text) so run_job
                    // can store them in tool_calls_json independently of result_text.
                    if let Some(ref tc) = tool_calls {
                        yield StreamToken { value: String::new(), is_final: false, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: Some(tc.clone()), finish_reason: None };
                    }
                    yield StreamToken { value: text, is_final: done, prompt_tokens, completion_tokens, cached_tokens, tool_calls: None, finish_reason };

                    if done {
                        return;
                    }
                }
            }

            // ── Flush remaining buffer ────────────────────────────────────────
            // If the final SSE event arrived without a trailing '\n' (the HTTP
            // response body ended mid-line), buf still holds the last JSON.
            // Parse it and emit a final token so run_job can complete the job.
            let line = buf.trim().to_string();
            if let Some(s) = line.strip_prefix("data:") {
                let s = s.trim();
                if !s.is_empty()
                    && let Ok(parsed) = serde_json::from_str::<GenerateResponse>(s) {
                        let text = extract_text(&parsed.candidates);
                        let (prompt_tokens, completion_tokens, cached_tokens) = extract_usage(&parsed);
                        let finish_reason = Some(map_finish_reason(&parsed.candidates).as_str().to_string());
                        yield StreamToken { value: text, is_final: true, prompt_tokens, completion_tokens, cached_tokens, tool_calls: None, finish_reason };
                        return;
                    }
            }

            // Stream ended without a finishReason event — emit empty done marker
            // so run_job marks the job as completed and the SSE client receives
            // a 'done' event.
            yield StreamToken::done();
        })
    }
}
