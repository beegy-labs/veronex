use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::domain::errors::DomainError;

/// Lightweight event fired on every job status transition.
/// Broadcast via tokio broadcast channel → SSE endpoint → network flow UI.
#[derive(Debug, Clone, Serialize)]
pub struct JobStatusEvent {
    pub id: String,
    pub status: String,
    pub model_name: String,
    pub provider_type: String,
    pub latency_ms: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct JobId(pub Uuid);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct ModelName(String);

impl ModelName {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if s.is_empty() {
            return Err(DomainError::Validation("model name cannot be empty".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct Prompt(String);

impl Prompt {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if s.is_empty() {
            return Err(DomainError::Validation("prompt cannot be empty".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct StreamToken {
    pub value: String,
    pub is_final: bool,
    /// Actual prompt token count from the provider's usage metadata.
    /// Only populated on the final token when the provider reports real counts.
    pub prompt_tokens: Option<u32>,
    /// Actual completion token count from the provider's usage metadata.
    /// Only populated on the final token when the provider reports real counts.
    pub completion_tokens: Option<u32>,
    /// Tokens served from cache (Gemini `cachedContentTokenCount`).
    /// Only populated on the final token; `None` for Ollama.
    pub cached_tokens: Option<u32>,
    /// Tool calls returned by the model (Ollama `/api/chat` format).
    /// When Some, this token carries tool call data instead of text content.
    /// Handlers must convert to the appropriate wire format (OpenAI vs Ollama NDJSON).
    pub tool_calls: Option<serde_json::Value>,
}

impl StreamToken {
    pub fn text(value: String) -> Self {
        Self { value, is_final: false, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None }
    }
    pub fn done() -> Self {
        Self { value: String::new(), is_final: true, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None }
    }
}

// ── Validated value objects (backend-only) ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Username(String);

impl Username {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if s.is_empty() {
            return Err(DomainError::Validation("username cannot be empty".to_string()));
        }
        if s.len() > 255 {
            return Err(DomainError::Validation("username exceeds 255 characters".to_string()));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(DomainError::Validation(
                "username must contain only alphanumeric characters, underscores, or hyphens".to_string(),
            ));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Email(String);

impl Email {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if !s.contains('@') {
            return Err(DomainError::Validation("email must contain '@'".to_string()));
        }
        if s.len() > 254 {
            return Err(DomainError::Validation("email exceeds 254 characters".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUrl(String);

impl ProviderUrl {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if !(s.starts_with("http://") || s.starts_with("https://")) {
            return Err(DomainError::Validation(
                "provider URL must start with http:// or https://".to_string(),
            ));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Username ─────────────────────────────────────────────────────────

    #[test]
    fn username_valid() {
        assert!(Username::new("alice").is_ok());
        assert!(Username::new("bob-123").is_ok());
        assert!(Username::new("c_d").is_ok());
    }

    #[test]
    fn username_empty() {
        assert!(Username::new("").is_err());
    }

    #[test]
    fn username_too_long() {
        let long = "a".repeat(256);
        assert!(Username::new(&long).is_err());
    }

    #[test]
    fn username_max_length_ok() {
        let max = "a".repeat(255);
        assert!(Username::new(&max).is_ok());
    }

    #[test]
    fn username_invalid_chars() {
        assert!(Username::new("no spaces").is_err());
        assert!(Username::new("bad@char").is_err());
        assert!(Username::new("hello!").is_err());
    }

    // ── Email ────────────────────────────────────────────────────────────

    #[test]
    fn email_valid() {
        assert!(Email::new("a@b.com").is_ok());
        assert!(Email::new("user@example.org").is_ok());
    }

    #[test]
    fn email_missing_at() {
        assert!(Email::new("noatsign.com").is_err());
    }

    #[test]
    fn email_too_long() {
        let long = format!("{}@b.com", "a".repeat(250));
        assert!(Email::new(&long).is_err());
    }

    #[test]
    fn email_max_length_ok() {
        let local = "a".repeat(246);
        let email = format!("{local}@b.c");
        assert_eq!(email.len(), 250);
        assert!(Email::new(&email).is_ok());
    }

    // ── ProviderUrl ──────────────────────────────────────────────────────

    #[test]
    fn provider_url_valid_http() {
        assert!(ProviderUrl::new("http://localhost:11434").is_ok());
    }

    #[test]
    fn provider_url_valid_https() {
        assert!(ProviderUrl::new("https://api.example.com").is_ok());
    }

    #[test]
    fn provider_url_missing_scheme() {
        assert!(ProviderUrl::new("localhost:11434").is_err());
    }

    #[test]
    fn provider_url_ftp_rejected() {
        assert!(ProviderUrl::new("ftp://files.example.com").is_err());
    }
}
