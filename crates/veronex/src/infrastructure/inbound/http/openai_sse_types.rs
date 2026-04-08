//! Shared OpenAI SSE response types used by multiple HTTP handlers.
//!
//! These structs model the `chat.completion.chunk` objects emitted over SSE
//! by the OpenAI-compatible streaming endpoints.

use serde::Serialize;

/// System fingerprint reported on all OpenAI-compat responses.
pub const SYSTEM_FINGERPRINT: &str = "fp_veronex";

/// Service tier reported on all OpenAI-compat responses.
pub const SERVICE_TIER_DEFAULT: &str = "default";

/// `stream_options` field in ChatCompletionRequest.
#[derive(serde::Deserialize)]
pub struct StreamOptions {
    pub include_usage: Option<bool>,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<&'static str>,
    pub choices: Vec<ChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

impl CompletionChunk {
    /// A content delta chunk (streaming text).
    pub fn content(id: String, created: i64, model: Option<String>, text: String) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            service_tier: Some(SERVICE_TIER_DEFAULT),
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent { content: Some(text), ..Default::default() },
                finish_reason: None,
            }],
            system_fingerprint: Some(SYSTEM_FINGERPRINT),
            usage: None,
        }
    }

    /// A finish chunk with a custom finish reason (end-of-stream sentinel).
    pub fn finish(id: String, created: i64, model: Option<String>, reason: &str) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            service_tier: Some(SERVICE_TIER_DEFAULT),
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent::default(),
                finish_reason: Some(reason.to_string()),
            }],
            system_fingerprint: Some(SYSTEM_FINGERPRINT),
            usage: None,
        }
    }

    /// A finish chunk with usage info attached (for `stream_options.include_usage`).
    pub fn finish_with_usage(
        id: String, created: i64, model: Option<String>, reason: &str, usage: UsageInfo,
    ) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            service_tier: Some(SERVICE_TIER_DEFAULT),
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent::default(),
                finish_reason: Some(reason.to_string()),
            }],
            system_fingerprint: Some(SYSTEM_FINGERPRINT),
            usage: Some(usage),
        }
    }

    /// A usage-only chunk with empty choices array (stream_options.include_usage final chunk).
    pub fn usage_only(id: String, created: i64, model: Option<String>, usage: UsageInfo) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            service_tier: Some(SERVICE_TIER_DEFAULT),
            choices: vec![],   // EMPTY - required by OpenAI spec
            system_fingerprint: Some(SYSTEM_FINGERPRINT),
            usage: Some(usage),
        }
    }

    /// A stop chunk (end-of-stream sentinel with `finish_reason: "stop"`).
    pub fn stop(id: String, created: i64, model: Option<String>) -> Self {
        use crate::domain::enums::FinishReason;
        Self::finish(id, created, model, FinishReason::Stop.as_str())
    }

    /// A tool-calls chunk.
    pub fn tool_calls(id: String, created: i64, model: Option<String>, calls: Vec<serde_json::Value>) -> Self {
        Self {
            id,
            object: "chat.completion.chunk",
            created,
            model,
            service_tier: Some(SERVICE_TIER_DEFAULT),
            choices: vec![ChunkChoice {
                index: 0,
                delta: DeltaContent { tool_calls: Some(calls), ..Default::default() },
                finish_reason: None,
            }],
            system_fingerprint: Some(SYSTEM_FINGERPRINT),
            usage: None,
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
    pub service_tier: &'static str,
    pub choices: Vec<CompletionChoice>,
    pub usage: UsageInfo,
    pub system_fingerprint: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conversation_renewed: bool,
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
    pub refusal: Option<serde_json::Value>,  // Always None/null per OpenAI spec
}

#[derive(Serialize, Clone, Default)]
pub struct PromptTokensDetails {
    pub cached_tokens: u32,
    pub audio_tokens: u32,
}

#[derive(Serialize, Clone, Default)]
pub struct CompletionTokensDetails {
    pub reasoning_tokens: u32,
    pub accepted_prediction_tokens: u32,
    pub rejected_prediction_tokens: u32,
    pub audio_tokens: u32,
}

#[derive(Serialize, Clone)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub prompt_tokens_details: PromptTokensDetails,
    pub completion_tokens_details: CompletionTokensDetails,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_chunk_sets_object_and_text() {
        let chunk = CompletionChunk::content("id1".into(), 42, Some("model1".into()), "hello".into());
        assert_eq!(chunk.id, "id1");
        assert_eq!(chunk.object, "chat.completion.chunk");
        assert_eq!(chunk.created, 42);
        assert_eq!(chunk.model.as_deref(), Some("model1"));
        assert_eq!(chunk.choices.len(), 1);
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("hello"));
        assert!(chunk.choices[0].finish_reason.is_none());
        assert!(chunk.usage.is_none());
    }

    #[test]
    fn finish_chunk_sets_reason_and_empty_delta() {
        let chunk = CompletionChunk::finish("id2".into(), 0, None, "stop");
        assert_eq!(chunk.choices.len(), 1);
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(chunk.choices[0].delta.content.is_none());
        assert!(chunk.choices[0].delta.tool_calls.is_none());
    }

    #[test]
    fn stop_chunk_uses_stop_reason() {
        let chunk = CompletionChunk::stop("id3".into(), 0, None);
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn usage_only_chunk_has_empty_choices() {
        let usage = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            prompt_tokens_details: PromptTokensDetails::default(),
            completion_tokens_details: CompletionTokensDetails::default(),
        };
        let chunk = CompletionChunk::usage_only("id4".into(), 0, None, usage);
        assert!(chunk.choices.is_empty(), "usage-only chunk must have empty choices per OpenAI spec");
        assert!(chunk.usage.is_some());
    }

    #[test]
    fn tool_calls_chunk_carries_calls_in_delta() {
        let calls = vec![serde_json::json!({"index": 0, "id": "call_0", "type": "function"})];
        let chunk = CompletionChunk::tool_calls("id5".into(), 0, None, calls.clone());
        assert_eq!(chunk.choices.len(), 1);
        let tc = chunk.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
    }

    #[test]
    fn finish_with_usage_embeds_both() {
        let usage = UsageInfo {
            prompt_tokens: 5,
            completion_tokens: 7,
            total_tokens: 12,
            prompt_tokens_details: PromptTokensDetails::default(),
            completion_tokens_details: CompletionTokensDetails::default(),
        };
        let chunk = CompletionChunk::finish_with_usage("id6".into(), 0, None, "length", usage);
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("length"));
        assert!(chunk.usage.is_some());
    }

}
