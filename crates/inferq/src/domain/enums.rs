use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    Api,
    Test,
}

/// Which API format the inbound request arrived via.
///
/// The discriminator is the matched route path — no header convention is used.
/// This is recorded on every queued job for per-API analytics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    /// POST /v1/chat/completions (OpenAI SDK, qwen-code, etc.)
    OpenaiCompat,
    /// POST /api/chat or /api/generate (OLLAMA_HOST=veronex clients)
    OllamaNative,
    /// POST /v1beta/models/{model}:generateContent (Gemini CLI, google-generativeai SDK)
    GeminiNative,
    /// POST /v1/inference (Veronex native SDK)
    VeronexNative,
}

impl Default for ApiFormat {
    fn default() -> Self {
        Self::OpenaiCompat
    }
}

impl Default for JobSource {
    fn default() -> Self {
        JobSource::Api
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    Ollama,
    Gemini,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinishReason {
    Stop,
    Length,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmBackendStatus {
    Online,
    Offline,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Loaded,
    Unloaded,
    Loading,
}
