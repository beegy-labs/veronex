use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Actual prompt token count from the backend's usage metadata.
    /// Only populated on the final token when the backend reports real counts.
    pub prompt_tokens: Option<u32>,
    /// Actual completion token count from the backend's usage metadata.
    /// Only populated on the final token when the backend reports real counts.
    pub completion_tokens: Option<u32>,
    /// Tokens served from cache (Gemini `cachedContentTokenCount`).
    /// Only populated on the final token; `None` for Ollama.
    pub cached_tokens: Option<u32>,
}
