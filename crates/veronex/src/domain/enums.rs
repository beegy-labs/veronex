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
pub enum ProviderType {
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
pub enum LlmProviderStatus {
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

/// Three-level thermal throttling state for GPU backends.
///
/// Updated by the health checker every 30 s; read by the queue dispatcher
/// on every job dispatch to gate concurrency limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThrottleLevel {
    /// Temp < 78 °C — full concurrency allowed.
    Normal,
    /// 78 °C ≤ temp < 92 °C — new slots allowed only if none active.
    Soft,
    /// temp ≥ 92 °C — no new slots until cooldown expires.
    Hard,
}
