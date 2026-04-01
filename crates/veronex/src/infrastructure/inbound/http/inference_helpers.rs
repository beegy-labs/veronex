//! Shared validation and helper functions for inference handler endpoints.
//!
//! Extracted from the duplicated logic across `openai_handlers`, `gemini_compat_handlers`,
//! and `ollama_compat_handlers` to provide a single source of truth for input validation
//! and common operations.

use std::convert::Infallible;

use axum::response::sse::Event;
use axum::response::Response;
use futures::StreamExt;

use crate::domain::value_objects::{ConvId, JobId, StreamToken};
use super::cancel_guard::CancelOnDrop;
use super::constants::{ERR_MODEL_INVALID, ERR_PROMPT_TOO_LARGE, MAX_MODEL_NAME_BYTES, MAX_PROMPT_BYTES};
use super::handlers::{SseStream, try_acquire_sse, sse_response};
use super::state::AppState;

// ── Header extraction ────────────────────────────────────────────────────────

/// Extract the `x-conversation-id` header value, if present and valid.
///
/// Decodes the `conv_{base62}` string from the header into a UUID.
/// Returns `None` when the header is absent, not valid UTF-8, exceeds 256 bytes,
/// or fails to decode.
pub fn extract_conversation_id(headers: &axum::http::HeaderMap) -> Option<uuid::Uuid> {
    headers
        .get("x-conversation-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.len() <= 256)
        .and_then(|s| decode_conversation_id(s))
}

/// Generate a new conversation ID as UUIDv7.
pub fn new_conversation_id() -> uuid::Uuid {
    uuid::Uuid::now_v7()
}

/// Encode a UUID as a prefixed base62 conversation ID (e.g. `"conv_3X4aB..."`).
pub fn to_public_id(uuid: &uuid::Uuid) -> String {
    ConvId::from_uuid(*uuid).to_string()
}

/// Decode a `conv_{base62}` conversation ID back to UUID.
pub fn decode_conversation_id(id: &str) -> Option<uuid::Uuid> {
    id.parse::<ConvId>().ok().map(|c| c.0)
}

// ── Input validation ────────────────────────────────────────────────────────

/// Validate that a model name is non-empty and within the byte limit.
pub fn validate_model_name(model: &str) -> Result<(), &'static str> {
    if model.is_empty() || model.len() > MAX_MODEL_NAME_BYTES {
        return Err(ERR_MODEL_INVALID);
    }
    Ok(())
}

/// Validate that the total content length does not exceed the prompt byte limit.
pub fn validate_content_length(total_bytes: usize) -> Result<(), &'static str> {
    if total_bytes > MAX_PROMPT_BYTES {
        return Err(ERR_PROMPT_TOO_LARGE);
    }
    Ok(())
}

// ── Image validation ────────────────────────────────────────────────────────

use crate::application::ports::outbound::lab_settings_repository::LabSettings;

/// Validate image count and compress oversized images to WebP.
///
/// - Images within `max_image_b64_bytes` are passed through unchanged (avoids
///   unnecessary re-encoding and double-lossy quality loss).
/// - Images exceeding the limit are resized to [`IMAGE_COMPRESS_MAX_EDGE`] + WebP.
///
/// Uses `spawn_blocking` for CPU-intensive image decode/resize/encode.
/// Returns error message on failure, None on success.
pub async fn validate_and_compress_images(images: &mut Option<Vec<String>>, lab: &LabSettings) -> Option<String> {
    let imgs = match images {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let max_count = lab.max_images_per_request as usize;
    if max_count == 0 {
        return Some("image input is disabled".into());
    }
    if imgs.len() > max_count {
        return Some(format!("too many images (max {max_count})"));
    }
    let max_bytes = lab.max_image_b64_bytes as usize;
    for img in imgs.iter_mut() {
        if img.len() > max_bytes {
            let b64 = img.clone();
            match tokio::task::spawn_blocking(move || {
                crate::infrastructure::outbound::s3::webp_convert::compress_base64_image(
                    &b64,
                    super::constants::IMAGE_COMPRESS_MAX_EDGE,
                )
            }).await {
                Ok(Ok(compressed)) => *img = compressed,
                Ok(Err(e)) => {
                    tracing::warn!("image compression failed, rejecting: {e}");
                    return Some("invalid image data".into());
                }
                Err(e) => {
                    tracing::warn!("image compression task panicked: {e}");
                    return Some("image processing failed".into());
                }
            }
        }
    }
    None
}

// ── Tool call validation (security) ─────────────────────────────────────────

/// Validate a single tool call from a provider response.
///
/// Checks that the tool call has a well-formed `function.name` field with
/// only safe characters. Rejects names with control characters or suspicious
/// patterns to prevent injection attacks (H4 security fix).
pub fn validate_tool_call(call: &serde_json::Value) -> bool {
    let func = match call.get("function") {
        Some(f) => f,
        None => return false,
    };
    let name = match func.get("name").and_then(|n| n.as_str()) {
        Some(n) if !n.is_empty() && n.len() <= 256 => n,
        _ => return false,
    };
    // Reject names with control characters or suspicious patterns
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-' || c == ':') {
        return false;
    }
    // Validate arguments size (max 64KB)
    if let Some(args) = func.get("arguments")
        && let Ok(s) = serde_json::to_string(args)
            && s.len() > 65_536 {
                return false;
            }
    true
}

// ── Prompt extraction ───────────────────────────────────────────────────────

/// Extract the last user message content from an Ollama-format messages array.
///
/// Scans messages in reverse to find the most recent `"role": "user"` entry
/// and returns its `"content"` field. Returns `"chat"` if no user message is
/// found (display-only fallback for InferenceJob).
pub fn extract_last_user_prompt(messages: &[serde_json::Value]) -> &str {
    messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("chat")
}

// ── SSE stream builder ───────────────────────────────────────────────────

/// Build an SSE response from a job's token stream with format-specific mapping.
///
/// Handles the shared boilerplate that was duplicated across `openai_handlers`
/// and `gemini_compat_handlers`:
/// 1. Acquire an SSE connection slot (429 on exhaustion)
/// 2. Subscribe to the job's token stream
/// 3. Map each token through the caller-provided `map_token` closure
/// 4. Optionally append a `[DONE]` sentinel (OpenAI convention)
/// 5. Wrap in `CancelOnDrop` so client disconnects free GPU resources
/// 6. Return via `sse_response()` (timeout + keep-alive + headers)
///
/// The `map_token` closure receives each `Result<StreamToken>` and returns
/// a `Vec<Event>`. Returning multiple events per token is needed for
/// `stream_options.include_usage` which emits a separate usage-only chunk
/// after the finish chunk. For all other cases return `vec![event]`.
pub fn build_sse_response(
    state: &AppState,
    job_id: JobId,
    append_done: bool,
    mut map_token: impl FnMut(Result<StreamToken, crate::domain::errors::DomainError>) -> Vec<Event> + Send + 'static,
) -> Response {
    let guard = match try_acquire_sse(&state.sse_connections) {
        Ok(g) => g,
        Err(resp) => return resp,
    };

    let token_stream = state.use_case.stream(&job_id);

    let content_stream = token_stream.flat_map(move |result| {
        let _ = &guard;
        let events = map_token(result);
        let results: Vec<Result<Event, Infallible>> = events.into_iter().map(Ok).collect();
        futures::stream::iter(results)
    });

    let sse_stream: SseStream = if append_done {
        let done = futures::stream::once(async {
            Ok::<_, Infallible>(Event::default().data("[DONE]"))
        });
        Box::pin(CancelOnDrop::new(
            content_stream.chain(done),
            job_id,
            state.use_case.clone(),
        ))
    } else {
        Box::pin(CancelOnDrop::new(
            content_stream,
            job_id,
            state.use_case.clone(),
        ))
    };

    sse_response(sse_stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Concrete examples kept as documentation.
    #[test]
    fn validate_model_name_examples() {
        assert!(validate_model_name("").is_err());
        assert!(validate_model_name("llama3.2:latest").is_ok());
    }

    #[test]
    fn validate_tool_call_examples() {
        let valid = serde_json::json!({"function": {"name": "get_weather", "arguments": {}}});
        assert!(validate_tool_call(&valid));
        let dotted = serde_json::json!({"function": {"name": "tools.weather:get-forecast", "arguments": {}}});
        assert!(validate_tool_call(&dotted));
        let missing = serde_json::json!({"name": "get_weather"});
        assert!(!validate_tool_call(&missing));
    }

    #[test]
    fn extract_last_user_prompt_finds_last() {
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "first"}),
            serde_json::json!({"role": "assistant", "content": "reply"}),
            serde_json::json!({"role": "user", "content": "second"}),
        ];
        assert_eq!(extract_last_user_prompt(&msgs), "second");
    }

    #[test]
    fn extract_last_user_prompt_fallback() {
        let msgs = vec![serde_json::json!({"role": "system", "content": "sys"})];
        assert_eq!(extract_last_user_prompt(&msgs), "chat");
    }

    proptest! {
        #[test]
        fn validate_model_name_within_limit_accepted(
            name in "[a-zA-Z0-9.:_-]{1,256}"
        ) {
            prop_assert!(validate_model_name(&name).is_ok());
        }

        #[test]
        fn validate_model_name_over_limit_rejected(extra in 1usize..500) {
            let name = "a".repeat(MAX_MODEL_NAME_BYTES + extra);
            prop_assert!(validate_model_name(&name).is_err());
        }

        #[test]
        fn validate_content_length_boundary(size in 0usize..=MAX_PROMPT_BYTES) {
            prop_assert!(validate_content_length(size).is_ok());
        }

        #[test]
        fn validate_content_length_over_limit_rejected(extra in 1usize..10000) {
            prop_assert!(validate_content_length(MAX_PROMPT_BYTES + extra).is_err());
        }

        #[test]
        fn validate_tool_call_safe_names_accepted(
            name in "[a-zA-Z][a-zA-Z0-9_.:_-]{0,50}"
        ) {
            let call = serde_json::json!({"function": {"name": name, "arguments": {}}});
            prop_assert!(validate_tool_call(&call));
        }

        #[test]
        fn validate_tool_call_control_chars_rejected(
            prefix in "[a-z]{1,5}",
            bad in "[\x00-\x1f]",
            suffix in "[a-z]{0,5}",
        ) {
            let name = format!("{prefix}{bad}{suffix}");
            let call = serde_json::json!({"function": {"name": name, "arguments": {}}});
            prop_assert!(!validate_tool_call(&call));
        }

    }

    #[tokio::test]
    async fn images_over_count_rejected() {
        let mut imgs: Option<Vec<String>> = Some((0..6).map(|_| "x".into()).collect());
        let lab = LabSettings { max_images_per_request: 4, ..LabSettings::default() };
        assert!(validate_and_compress_images(&mut imgs, &lab).await.is_some());
    }

    #[tokio::test]
    async fn validate_images_none_passes() {
        assert!(validate_and_compress_images(&mut None, &LabSettings::default()).await.is_none());
    }

    #[tokio::test]
    async fn validate_images_disabled_rejected() {
        let lab = LabSettings { max_images_per_request: 0, ..LabSettings::default() };
        assert!(validate_and_compress_images(&mut Some(vec!["abc".into()]), &lab).await.is_some());
    }

    #[tokio::test]
    async fn validate_images_oversized_invalid_data_rejected() {
        let lab = LabSettings { max_image_b64_bytes: 10, ..LabSettings::default() };
        let mut imgs = Some(vec!["x".repeat(20)]);
        // Exceeds max_bytes → compression attempted → invalid data → rejected
        assert!(validate_and_compress_images(&mut imgs, &lab).await.is_some());
    }
}
