use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;

use crate::domain::entities::{InferenceJob, InferenceResult};
use crate::domain::value_objects::StreamToken;

/// Outbound port for a single LLM inference provider (Ollama, Gemini, …).
///
/// **Precondition (Phase 1)**: callers SHOULD invoke
/// [`crate::application::ports::outbound::model_lifecycle::ModelLifecyclePort::ensure_ready`]
/// before `stream_tokens` / `infer` to guarantee the model is warm. Implicit
/// auto-load behavior in adapters (single 300 s `PROVIDER_REQUEST_TIMEOUT`)
/// is retained as defense-in-depth but should not be relied upon for correct
/// timing — cold-load on 200K-context models can exceed 160 s and conflate
/// load-failure with inference-failure observability. See SDD
/// `.specs/veronex/inference-lifecycle-sod.md`.
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
