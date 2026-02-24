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

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
    done: bool,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
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
        let url = format!("{}/api/generate", self.base_url);
        let client = self.client.clone();
        let model = job.model_name.as_str().to_string();
        let prompt = job.prompt.as_str().to_string();

        Box::pin(async_stream::try_stream! {
            let response = client
                .post(&url)
                .json(&serde_json::json!({
                    "model": model,
                    "prompt": prompt,
                    "stream": true,
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
                        .map_err(|e| anyhow::anyhow!("failed to parse Ollama response: {e}: {line}"))?;

                    yield StreamToken {
                        value: chunk.response,
                        is_final: chunk.done,
                    };

                    if chunk.done {
                        return;
                    }
                }
            }
        })
    }
}
