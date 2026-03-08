use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ── Account role ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum AccountRole {
    Super,
    Admin,
}

impl AccountRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Super => "super",
            Self::Admin => "admin",
        }
    }
}

impl std::fmt::Display for AccountRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for AccountRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "super" => Ok(Self::Super),
            "admin" => Ok(Self::Admin),
            other => Err(format!("invalid role: {other}")),
        }
    }
}

// ── Job / API enums ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    #[default]
    Api,
    Test,
}

/// Which API format the inbound request arrived via.
///
/// The discriminator is the matched route path — no header convention is used.
/// This is recorded on every queued job for per-API analytics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    /// POST /v1/chat/completions (OpenAI SDK, qwen-code, etc.)
    #[default]
    OpenaiCompat,
    /// POST /api/chat or /api/generate (OLLAMA_HOST=veronex clients)
    OllamaNative,
    /// POST /v1beta/models/{model}:generateContent (Gemini CLI, google-generativeai SDK)
    GeminiNative,
    /// POST /v1/inference (Veronex native SDK)
    VeronexNative,
}

impl ApiFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenaiCompat => "openai_compat",
            Self::OllamaNative => "ollama_native",
            Self::GeminiNative => "gemini_native",
            Self::VeronexNative => "veronex_native",
        }
    }
}

impl std::str::FromStr for ApiFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "openai_compat" => Ok(Self::OpenaiCompat),
            "ollama_native" => Ok(Self::OllamaNative),
            "gemini_native" => Ok(Self::GeminiNative),
            "veronex_native" => Ok(Self::VeronexNative),
            _ => Err(format!("unknown ApiFormat: {s}")),
        }
    }
}


impl JobSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Test => "test",
        }
    }
}

impl std::str::FromStr for JobSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "api" => Ok(Self::Api),
            "test" => Ok(Self::Test),
            other => Err(format!("invalid job source: {other}")),
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Ollama,
    Gemini,
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Gemini => "gemini",
        }
    }

    /// Audit trail resource type string for this provider type.
    pub fn resource_type(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama_provider",
            Self::Gemini => "gemini_provider",
        }
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ollama" => Ok(Self::Ollama),
            "gemini" => Ok(Self::Gemini),
            other => Err(format!("unknown provider type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum FinishReason {
    Stop,
    Length,
    Cancelled,
    Error,
}

impl FinishReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for JobStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("unknown job status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum LlmProviderStatus {
    Online,
    Offline,
    Degraded,
}

impl LlmProviderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Degraded => "degraded",
        }
    }
}

/// Three-level thermal throttling state for GPU providers.
///
/// Updated by the health checker every 30 s; read by the queue dispatcher
/// on every job dispatch to gate concurrency limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleLevel {
    /// Temp < 78 °C — full concurrency allowed.
    Normal,
    /// 78 °C ≤ temp < 92 °C — new slots allowed only if none active.
    Soft,
    /// temp ≥ 92 °C — no new slots until cooldown expires.
    Hard,
}

// ── API Key enums ────────────────────────────────────────────────────────────

/// Billing tier for an API key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum KeyTier {
    Free,
    #[default]
    Paid,
}

impl KeyTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Paid => "paid",
        }
    }
}


impl std::fmt::Display for KeyTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for KeyTier {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "free" => Ok(Self::Free),
            "paid" => Ok(Self::Paid),
            other => Err(format!("invalid tier: {other}")),
        }
    }
}

/// API key category: standard (production) or test (dev/testing).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum KeyType {
    #[default]
    Standard,
    Test,
}

impl KeyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Test => "test",
        }
    }

    pub fn is_test(&self) -> bool {
        matches!(self, Self::Test)
    }
}


impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for KeyType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "test" => Ok(Self::Test),
            other => Err(format!("invalid key_type: {other}")),
        }
    }
}
