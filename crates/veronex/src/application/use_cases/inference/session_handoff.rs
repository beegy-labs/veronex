//! Phase 6 — Session Handoff.
//!
//! When the total compressed token count in a conversation exceeds
//! `handoff_threshold × configured_ctx`, create a new session with a
//! HandoffTurn (master summary) as its first turn.

use std::sync::Arc;

use uuid::Uuid;
use chrono::NaiveDate;

use crate::application::ports::outbound::lab_settings_repository::LabSettings;
use crate::application::ports::outbound::message_store::{
    ConversationRecord, ConversationTurn, HandoffTurn, MessageStore,
};

const HANDOFF_SUMMARY_PROMPT: &str = "You are a conversation summarizer. Given a multi-turn \
conversation history below, write a concise master summary (~300 tokens) that a new session \
can use as context. Preserve: key decisions, outcomes, user goals, errors encountered, \
named entities, and any open questions. Output ONLY the summary. No preamble.";

// ── Token counting ────────────────────────────────────────────────────────────

fn count_record_tokens(record: &ConversationRecord) -> u32 {
    record.regular_turns().map(|t| {
        let p = t.compressed.as_ref()
            .map(|c| c.compressed_tokens)
            .unwrap_or_else(|| (t.prompt.len() / 4) as u32);
        let r = t.result.as_deref()
            .map(|s| (s.len() / 4) as u32)
            .unwrap_or(0);
        p + r
    }).sum()
}

// ── Handoff check ─────────────────────────────────────────────────────────────

/// Returns `true` when the conversation should be handed off to a new session.
pub fn should_handoff(
    record: &ConversationRecord,
    configured_ctx: u32,
    lab: &LabSettings,
) -> bool {
    if !lab.handoff_enabled || configured_ctx == 0 {
        return false;
    }
    let total_tokens = count_record_tokens(record);
    let threshold = (configured_ctx as f32 * lab.handoff_threshold) as u32;
    total_tokens >= threshold
}

// ── Master summary generation ─────────────────────────────────────────────────

async fn generate_master_summary(
    http: &reqwest::Client,
    record: &ConversationRecord,
    model: &str,
    provider_url: &str,
    timeout_secs: u64,
) -> anyhow::Result<String> {
    // Build conversation text for summarisation
    let history: String = record.regular_turns()
        .map(|t| {
            let resp = t.result.as_deref().unwrap_or("(no response)");
            format!("User: {}\nAssistant: {}\n", t.prompt, resp)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let endpoint = format!("{}/api/chat", provider_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": HANDOFF_SUMMARY_PROMPT },
            { "role": "user",   "content": history }
        ],
        "stream": false,
        "options": { "temperature": 0.0, "num_predict": 500 }
    });

    let resp = http
        .post(&endpoint)
        .json(&body)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Ollama returned {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await?;
    let summary = json["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if summary.is_empty() {
        anyhow::bail!("empty summary from model");
    }

    Ok(summary)
}

// ── Perform handoff ───────────────────────────────────────────────────────────

/// Perform session handoff: generate master summary, create new conversation
/// with HandoffTurn as first turn, return `(new_conversation_id, master_summary)`.
///
/// Non-fatal: returns `None` on any error (caller continues with original session).
#[allow(clippy::too_many_arguments)]
pub async fn perform_handoff(
    http: &reqwest::Client,
    record: &ConversationRecord,
    previous_conversation_id: Uuid,
    owner_id: Uuid,
    date: NaiveDate,
    model: &str,
    provider_url: &str,
    timeout_secs: u64,
    store: &Arc<dyn MessageStore>,
) -> Option<(Uuid, String)> {
    let previous_turn_count = record.regular_turns().count() as u32;

    let master_summary = match generate_master_summary(http, record, model, provider_url, timeout_secs).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                conv_id = %previous_conversation_id,
                "session_handoff: master summary failed (non-fatal): {e}"
            );
            return None;
        }
    };

    let new_conv_id = uuid::Uuid::now_v7();
    let handoff_turn = HandoffTurn {
        master_summary: master_summary.clone(),
        summary_model: model.to_string(),
        previous_conversation_id,
        previous_turn_count,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let new_record = ConversationRecord {
        turns: vec![ConversationTurn::Handoff(handoff_turn)],
    };

    if let Err(e) = store.put_conversation(owner_id, date, new_conv_id, &new_record).await {
        tracing::warn!(
            new_conv_id = %new_conv_id,
            "session_handoff: put_conversation failed (non-fatal): {e}"
        );
        return None;
    }

    tracing::info!(
        previous_conv_id = %previous_conversation_id,
        new_conv_id = %new_conv_id,
        previous_turn_count,
        "session_handoff: new session created"
    );

    Some((new_conv_id, master_summary))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::outbound::message_store::{
        CompressedTurn, ConversationRecord, ConversationTurn, TurnRecord,
    };

    fn make_turn(prompt: &str, result: &str, compressed_tokens: Option<u32>) -> TurnRecord {
        TurnRecord {
            job_id: Uuid::now_v7(),
            prompt: prompt.to_string(),
            messages: None,
            tool_calls: None,
            result: Some(result.to_string()),
            model_name: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            compressed: compressed_tokens.map(|ct| CompressedTurn {
                summary: "summary".to_string(),
                original_tokens: ct * 3,
                compressed_tokens: ct,
                compression_model: "qwen2.5:3b".to_string(),
            }),
            vision_analysis: None,
        }
    }

    fn make_record(turns: Vec<TurnRecord>) -> ConversationRecord {
        ConversationRecord {
            turns: turns.into_iter().map(ConversationTurn::Regular).collect(),
        }
    }

    fn lab(enabled: bool, threshold: f32) -> LabSettings {
        LabSettings {
            handoff_enabled: enabled,
            handoff_threshold: threshold,
            ..Default::default()
        }
    }

    #[test]
    fn handoff_disabled_returns_false() {
        let record = make_record(vec![make_turn("q", "a", Some(8_000))]);
        assert!(!should_handoff(&record, 16_384, &lab(false, 0.5)));
    }

    #[test]
    fn zero_ctx_returns_false() {
        let record = make_record(vec![make_turn("q", "a", Some(8_000))]);
        assert!(!should_handoff(&record, 0, &lab(true, 0.5)));
    }

    #[test]
    fn below_threshold_returns_false() {
        // 4 turns × 100 compressed_tokens = 400 tokens; threshold = 0.8 × 1000 = 800
        let turns = (0..4).map(|_| make_turn("question", "answer", Some(100))).collect();
        let record = make_record(turns);
        assert!(!should_handoff(&record, 1_000, &lab(true, 0.8)));
    }

    #[test]
    fn at_threshold_triggers_handoff() {
        // 8 turns × 100 compressed_tokens = 800 tokens; threshold = 0.8 × 1000 = 800
        let turns = (0..8).map(|_| make_turn("question", "answer", Some(100))).collect();
        let record = make_record(turns);
        assert!(should_handoff(&record, 1_000, &lab(true, 0.8)));
    }

    #[test]
    fn above_threshold_triggers_handoff() {
        // 10 turns × 100 compressed_tokens = 1000; threshold = 0.8 × 1000 = 800
        let turns = (0..10).map(|_| make_turn("question", "answer", Some(100))).collect();
        let record = make_record(turns);
        assert!(should_handoff(&record, 1_000, &lab(true, 0.8)));
    }

    #[test]
    fn uncompressed_turns_use_char_estimate() {
        // 400-char prompt + 400-char result → (400+400)/4 = 200 tokens per turn
        // 4 turns = 800 tokens; threshold = 0.8 × 1000 = 800
        let prompt = "a".repeat(400);
        let result = "b".repeat(400);
        let turns = (0..4).map(|_| make_turn(&prompt, &result, None)).collect();
        let record = make_record(turns);
        assert!(should_handoff(&record, 1_000, &lab(true, 0.8)));
    }

    #[test]
    fn handoff_turn_skipped_in_token_count() {
        // HandoffTurn should not contribute to token count
        let mut record = make_record(vec![make_turn("q", "a", Some(100))]);
        record.turns.insert(
            0,
            ConversationTurn::Handoff(HandoffTurn {
                master_summary: "x".repeat(10_000),
                summary_model: "qwen2.5:3b".to_string(),
                previous_conversation_id: Uuid::now_v7(),
                previous_turn_count: 5,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }),
        );
        // Only 100 tokens from the regular turn; threshold = 0.8 × 1000 = 800
        assert!(!should_handoff(&record, 1_000, &lab(true, 0.8)));
    }
}
