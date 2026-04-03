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

    let http = reqwest::Client::new();
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
pub async fn perform_handoff(
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

    let master_summary = match generate_master_summary(record, model, provider_url, timeout_secs).await {
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
