use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;

use crate::domain::entities::{InferenceJob, InferenceResult};
use crate::domain::value_objects::StreamToken;

/// Outbound port for a single LLM inference provider (Ollama, Gemini, …).
#[async_trait]
pub trait InferenceProviderPort: Send + Sync {
    /// Non-streaming inference — returns when the full response is ready.
    async fn infer(&self, job: &InferenceJob) -> Result<InferenceResult>;

    /// Streaming inference — yields tokens as they are generated.
    fn stream_tokens(
        &self,
        job: &InferenceJob,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>;
}
