//! Token counter + context budget gate.
//!
//! SDD: `.specs/veronex/history/conversation-context-compression.md` §4 (Tier B).
//!
//! Used by the pre-flight pruner (Tier C) to decide whether the accumulated
//! `messages[]` array fits under the model's effective budget before calling
//! the LLM.
//!
//! Rationale for `cl100k_base`: Ollama provides no `/tokenize` endpoint, and
//! per-call LLM round-trips just to count tokens are too expensive at the
//! gateway-loop hot path. `cl100k_base` is the OpenAI tokenizer; for Qwen /
//! Llama families it produces counts within ±10% of the true tokenizer.
//! With our 95% threshold + 0.75 conservative budget ratio, a 10% drift can
//! trim slightly earlier than necessary but cannot cause overflow.

use std::sync::OnceLock;

use serde_json::Value;
use tiktoken_rs::{cl100k_base, CoreBPE};

/// Per-message overhead per OpenAI's tokenizer FAQ — role + delimiter
/// scaffolding the model adds around each chat message.
const PER_MESSAGE_TOKENS: u32 = 4;

/// Priming tokens added once per request (assistant primer).
const PRIMING_TOKENS: u32 = 2;

/// Safety margin subtracted from the raw budget — protects against the ±10%
/// drift of `cl100k_base` vs the model's true tokenizer.
const BUDGET_SAFETY_MARGIN_TOKENS: u32 = 1024;

/// Lazy-initialized tokenizer; the BPE table load takes a few ms which we
/// want to amortize across calls.
fn bpe() -> Option<&'static CoreBPE> {
    static BPE: OnceLock<Option<CoreBPE>> = OnceLock::new();
    BPE.get_or_init(|| cl100k_base().ok()).as_ref()
}

/// Estimate-by-character-count fallback when the tokenizer fails to
/// initialize (e.g. corrupted BPE blob in deployment). Loose ~4 chars/token
/// heuristic — wildly inaccurate for CJK so this is genuinely a last resort.
fn fallback_estimate(messages: &[Value]) -> u32 {
    let mut chars: u32 = 0;
    for m in messages {
        if let Some(role) = m.get("role").and_then(Value::as_str) {
            chars = chars.saturating_add(role.len() as u32);
        }
        if let Some(content) = m.get("content").and_then(Value::as_str) {
            chars = chars.saturating_add(content.len() as u32);
        }
        chars = chars.saturating_add(PER_MESSAGE_TOKENS);
    }
    chars.saturating_add(PRIMING_TOKENS).max(1) / 4
}

/// Count tokens in a chat-style messages array.
///
/// Counts both `role` and `content` fields (top-level strings) plus the
/// per-message scaffolding overhead. Tool-call payloads and nested image
/// content are NOT counted — they're rare on the gateway hot path and the
/// pre-flight only needs an upper-bound that triggers compression slightly
/// early rather than too late.
pub fn count_messages_tokens(messages: &[Value]) -> u32 {
    let Some(bpe) = bpe() else {
        return fallback_estimate(messages);
    };
    let mut total: u32 = 0;
    for m in messages {
        if let Some(role) = m.get("role").and_then(Value::as_str) {
            total = total.saturating_add(bpe.encode_with_special_tokens(role).len() as u32);
        }
        if let Some(content) = m.get("content").and_then(Value::as_str) {
            total = total.saturating_add(bpe.encode_with_special_tokens(content).len() as u32);
        }
        total = total.saturating_add(PER_MESSAGE_TOKENS);
    }
    total.saturating_add(PRIMING_TOKENS)
}

/// Compute the in-memory token budget given the model's configured context
/// window and an operator-tunable ratio (`lab_settings.context_budget_ratio`,
/// default 0.75). Subtracts a safety margin for tokenizer drift.
///
/// Returned value is the maximum number of tokens the message array may
/// occupy before the pruner must trim. Always >= 0; saturates at 0 if the
/// configured ctx is smaller than the safety margin (a misconfiguration but
/// defensively handled).
pub fn budget_for_context(configured_ctx: u32, ratio: f32) -> u32 {
    let raw = (configured_ctx as f32 * ratio.clamp(0.0, 1.0)) as u32;
    raw.saturating_sub(BUDGET_SAFETY_MARGIN_TOKENS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user(content: &str) -> Value {
        json!({"role": "user", "content": content})
    }
    fn assistant(content: &str) -> Value {
        json!({"role": "assistant", "content": content})
    }
    fn system(content: &str) -> Value {
        json!({"role": "system", "content": content})
    }

    // ── count_messages_tokens ──────────────────────────────────────────────

    #[test]
    fn empty_messages_returns_priming_only() {
        // 0 messages × per_message_overhead + priming = 2
        let n = count_messages_tokens(&[]);
        assert_eq!(n, PRIMING_TOKENS);
    }

    #[test]
    fn single_short_message_within_reasonable_range() {
        let n = count_messages_tokens(&[user("Hello, world!")]);
        // role(1) + content(~4) + per_msg(4) + priming(2) = ~11
        // Tight upper/lower bounds because this isn't a real-content test.
        assert!((9..=20).contains(&n), "got {n}");
    }

    #[test]
    fn count_scales_with_content_length() {
        let short = count_messages_tokens(&[user("hi")]);
        let long = count_messages_tokens(&[user(&"hi ".repeat(500))]);
        assert!(long > short * 50, "long={long} short={short}");
    }

    #[test]
    fn multiple_messages_aggregate() {
        let a = count_messages_tokens(&[user("hello")]);
        let b = count_messages_tokens(&[user("hello"), assistant("hi")]);
        // b > a by at least the second message's overhead + role + content
        assert!(b > a + PER_MESSAGE_TOKENS, "a={a} b={b}");
    }

    #[test]
    fn missing_content_field_does_not_panic() {
        // Tool-call assistant messages omit content (only have tool_calls).
        let msg = json!({"role": "assistant"});
        let n = count_messages_tokens(&[msg]);
        // role + per_msg + priming
        assert!(n >= PER_MESSAGE_TOKENS + PRIMING_TOKENS);
    }

    #[test]
    fn non_string_content_does_not_panic() {
        // Multimodal content can be an array; we just skip it (rare on hot path).
        let msg = json!({"role": "user", "content": [{"type": "text", "text": "hi"}]});
        let n = count_messages_tokens(&[msg]);
        // role only — content array ignored gracefully
        assert!(n >= PER_MESSAGE_TOKENS + PRIMING_TOKENS);
    }

    #[test]
    fn role_and_content_both_counted() {
        // Ensure neither role nor content is double-counted or skipped.
        let with_both = count_messages_tokens(&[user("xyz")]);
        let role_only = count_messages_tokens(&[json!({"role": "user"})]);
        assert!(with_both > role_only);
    }

    #[test]
    fn realistic_conversation_under_4k() {
        let convo = vec![
            system("You are a helpful assistant."),
            user("What's the capital of France?"),
            assistant("The capital of France is Paris."),
            user("And of Germany?"),
            assistant("Berlin is the capital of Germany."),
        ];
        let n = count_messages_tokens(&convo);
        // 5 messages of short content — should be << 200 tokens
        assert!((30..=200).contains(&n), "got {n}");
    }

    // ── budget_for_context ─────────────────────────────────────────────────

    #[test]
    fn budget_at_75_percent_minus_safety_margin() {
        let b = budget_for_context(262_144, 0.75);
        // 75% of 262_144 = 196_608, minus 1024 safety = 195_584
        assert_eq!(b, 195_584);
    }

    #[test]
    fn budget_at_default_ollama_8k() {
        let b = budget_for_context(8192, 0.75);
        // 75% of 8192 = 6144, minus 1024 = 5120
        assert_eq!(b, 5120);
    }

    #[test]
    fn budget_clamps_ratio_above_1() {
        // A misconfigured 1.5 ratio shouldn't double the budget.
        let b = budget_for_context(10_000, 1.5);
        // clamps to 1.0 → 10_000 - 1024 = 8976
        assert_eq!(b, 8976);
    }

    #[test]
    fn budget_clamps_ratio_below_0() {
        let b = budget_for_context(10_000, -0.1);
        // clamps to 0.0 → 0, saturating sub of 1024 = 0
        assert_eq!(b, 0);
    }

    #[test]
    fn budget_handles_tiny_ctx_without_underflow() {
        // ctx smaller than safety margin → 0, not panic / underflow
        let b = budget_for_context(512, 0.75);
        assert_eq!(b, 0);
    }
}
