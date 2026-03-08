//! Shared OpenAI SSE response types used by multiple HTTP handlers.
//!
//! These structs model the `chat.completion.chunk` objects emitted over SSE
//! by the OpenAI-compatible streaming endpoints.

use serde::Serialize;

/// A single delta payload within a streaming chunk choice.
///
/// Optional fields are annotated with `skip_serializing_if` so that only
/// the fields relevant to a particular event are included in the JSON.
#[derive(Serialize, Default)]
pub struct DeltaContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

/// One choice entry inside a `CompletionChunk`.
#[derive(Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: DeltaContent,
    pub finish_reason: Option<String>,
}

/// A single SSE frame in the OpenAI streaming chat completion format.
#[derive(Serialize)]
pub struct CompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<ChunkChoice>,
}

impl CompletionChunk {
    /// A content delta chunk (streaming text).
    pub fn content(id: String, created: i64, model: Option<String>, text: String) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent { content: Some(text), ..Default::default() },
                finish_reason: None,
            }],
        }
    }

    /// A finish chunk with a custom finish reason (end-of-stream sentinel).
    pub fn finish(id: String, created: i64, model: Option<String>, reason: &str) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent::default(),
                finish_reason: Some(reason.to_string()),
            }],
        }
    }

    /// A stop chunk (end-of-stream sentinel with `finish_reason: "stop"`).
    pub fn stop(id: String, created: i64, model: Option<String>) -> Self {
        Self::finish(id, created, model, "stop")
    }

    /// A tool-calls chunk.
    pub fn tool_calls(id: String, created: i64, model: Option<String>, calls: Vec<serde_json::Value>) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent { tool_calls: Some(calls), ..Default::default() },
                finish_reason: None,
            }],
        }
    }
}

// ── Non-streaming response types ─────────────────────────────────────────────

/// Non-streaming `ChatCompletion` response (OpenAI format).
#[derive(Serialize)]
pub struct ChatCompletion {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub model: String,
    pub choices: Vec<CompletionChoice>,
    pub usage: UsageInfo,
}

#[derive(Serialize)]
pub struct CompletionChoice {
    pub index: u32,
    pub message: CompletionMessage,
    pub finish_reason: String,
}

#[derive(Serialize)]
pub struct CompletionMessage {
    pub role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
