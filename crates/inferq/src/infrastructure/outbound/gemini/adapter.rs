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

const BASE_URL: &str = "https://generativelanguage.googleapis.com";

pub struct GeminiAdapter {
    api_key: String,
    client: reqwest::Client,
}

impl GeminiAdapter {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
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
}

#[derive(Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
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

fn is_done(candidates: &[Candidate]) -> bool {
    candidates
        .first()
        .and_then(|c| c.finish_reason.as_deref())
        .is_some_and(|r| !r.is_empty())
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

// ── InferenceBackendPort impl ──────────────────────────────────────────────────

#[async_trait]
impl InferenceBackendPort for GeminiAdapter {
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult> {
        let start = Instant::now();
        let model = job.model_name.as_str();
        let url = format!(
            "{BASE_URL}/v1beta/models/{model}:generateContent?key={}",
            self.api_key
        );

        let body = GenerateRequest::new(job.prompt.as_str());
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
        let text = extract_text(&resp.candidates);
        let finish_reason = map_finish_reason(&resp.candidates);

        Ok(InferenceResult {
            job_id: job.id.clone(),
            prompt_tokens: resp
                .usage_metadata
                .as_ref()
                .and_then(|u| u.prompt_token_count)
                .unwrap_or(0),
            completion_tokens: resp
                .usage_metadata
                .as_ref()
                .and_then(|u| u.candidates_token_count)
                .unwrap_or(0),
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
            let url = format!(
                "{BASE_URL}/v1beta/models/{model}:streamGenerateContent?key={api_key}&alt=sse"
            );
            let body = GenerateRequest::new(&prompt);

            let response = client.post(&url).json(&body).send().await?;

            let status = response.status();
            if !status.is_success() {
                Err(anyhow::anyhow!("Gemini returned {status}"))?;
            }

            let mut byte_stream = response.bytes_stream();
            let mut buf = String::new();

            while let Some(chunk) = byte_stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!(e))?;
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete lines from the SSE stream.
                // Gemini SSE lines look like:  "data: {...json...}"
                while let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim().to_string();
                    buf = buf[nl + 1..].to_string();

                    let json_str = match line.strip_prefix("data:") {
                        Some(s) => s.trim(),
                        None => continue, // skip blank / comment lines
                    };

                    let chunk: GenerateResponse = serde_json::from_str(json_str)
                        .map_err(|e| anyhow::anyhow!("failed to parse Gemini response: {e}: {json_str}"))?;

                    let text = extract_text(&chunk.candidates);
                    let done = is_done(&chunk.candidates);

                    yield StreamToken { value: text, is_final: done };

                    if done {
                        return;
                    }
                }
            }
        })
    }
}
