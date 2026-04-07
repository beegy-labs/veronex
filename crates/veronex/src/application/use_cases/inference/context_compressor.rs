use std::sync::Arc;
use uuid::Uuid;

use anyhow::Result;
use chrono::NaiveDate;

use crate::application::ports::outbound::message_store::{CompressedTurn, MessageStore};
use crate::application::ports::outbound::valkey_port::ValkeyPort;

use super::compression_router::CompressParams;

// ── Prompt ───────────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = "You are a lossless context compressor. Summarize the following \
conversation turn into a single compact paragraph. Rules:\n\
- Preserve: intent of question, key decisions, named entities, numbers, errors, code identifiers.\n\
- Omit: filler, repetition, courtesy phrases.\n\
- Output ONLY the summary. No preamble. No labels.\n\
- Target: under 120 words.";

// ── Public entry point ───────────────────────────────────────────────────────

/// Compress the turn for `job_id` in the given conversation and write it back to S3.
///
/// Non-fatal: all errors are logged as warnings. Never panics.
/// Reads ConversationRecord from Valkey (if available) or S3, updates the
/// `TurnRecord.compressed` field, re-writes to S3, then DELs the Valkey cache
/// so the next read picks up the compressed data.
pub async fn compress_turn(
    params: &CompressParams,
    job_id: Uuid,
    owner_id: Uuid,
    date: NaiveDate,
    conversation_id: Uuid,
    store: Arc<dyn MessageStore>,
    valkey: Option<Arc<dyn ValkeyPort>>,
) {
    if let Err(e) = try_compress(
        params, job_id, owner_id, date, conversation_id, &store, &valkey,
    )
    .await
    {
        tracing::warn!(
            job_id = %job_id,
            model = %params.model,
            "compress_turn failed (non-fatal): {e}"
        );
    }
}

// ── Core implementation ──────────────────────────────────────────────────────

async fn try_compress(
    params: &CompressParams,
    job_id: Uuid,
    owner_id: Uuid,
    date: NaiveDate,
    conversation_id: Uuid,
    store: &Arc<dyn MessageStore>,
    valkey: &Option<Arc<dyn ValkeyPort>>,
) -> Result<()> {
    let cache_key =
        crate::infrastructure::outbound::valkey_keys::conversation_record(conversation_id);

    // 1. Load ConversationRecord — Valkey first, S3 fallback
    let record_opt: Option<crate::application::ports::outbound::message_store::ConversationRecord> =
        if let Some(vk) = valkey {
            vk.kv_get(&cache_key)
                .await
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str(&json).ok())
        } else {
            None
        };

    let mut record = match record_opt {
        Some(r) => r,
        None => store
            .get_conversation(owner_id, date, conversation_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("conversation not found in S3"))?,
    };

    // 2. Find the TurnRecord to compress
    let turn = record
        .turns
        .iter_mut()
        .filter_map(|t| t.as_regular_mut())
        .find(|t| t.job_id == job_id)
        .ok_or_else(|| anyhow::anyhow!("turn {job_id} not found in record"))?;

    if turn.compressed.is_some() {
        return Ok(()); // already compressed (e.g. retry after prior success)
    }

    let prompt = turn.prompt.clone();
    let result = turn.result.clone().unwrap_or_default();
    let original_tokens = (estimate_tokens(&prompt) + estimate_tokens(&result)) as u32;

    // 3. Call Ollama /api/chat for per-turn compression
    let http = reqwest::Client::new();
    let endpoint = format!("{}/api/chat", params.provider_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": params.model,
        "messages": [
            { "role": "system",  "content": SYSTEM_PROMPT },
            { "role": "user",    "content": format!("Q: {prompt}\nA: {result}") }
        ],
        "stream": false,
        "options": { "temperature": 0.0, "num_predict": 200 }
    });

    let resp = http
        .post(&endpoint)
        .json(&body)
        .timeout(std::time::Duration::from_secs(params.timeout_secs))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Ollama compression API returned HTTP {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await?;
    let summary = json["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if summary.is_empty() {
        anyhow::bail!("compression model returned empty summary");
    }

    let compressed_tokens = estimate_tokens(&summary) as u32;

    tracing::debug!(
        job_id = %job_id,
        original_tokens,
        compressed_tokens,
        model = %params.model,
        ratio = format!("{:.1}x", original_tokens as f32 / compressed_tokens.max(1) as f32),
        "turn compressed"
    );

    // 4. Store CompressedTurn in the TurnRecord
    turn.compressed = Some(CompressedTurn {
        summary,
        original_tokens,
        compressed_tokens,
        compression_model: params.model.clone(),
    });

    // 5. Re-write ConversationRecord to S3
    store
        .put_conversation(owner_id, date, conversation_id, &record)
        .await?;

    // 6. DEL Valkey cache — next reader will load the fresh S3 record with compressed data
    if let Some(vk) = valkey {
        if let Err(e) = vk.kv_del(&cache_key).await {
            tracing::warn!(key = %cache_key, error = %e, "context_compressor: failed to invalidate Valkey cache");
        }
    }

    Ok(())
}

// ── Token estimate ────────────────────────────────────────────────────────────

/// Rough token estimate: chars / 4.
/// Known limitation: underestimates CJK content.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

// ── Phase 5: Inline input compression ────────────────────────────────────────

const INPUT_COMPRESS_PROMPT: &str = "You are a lossless prompt compressor. Condense the user input \
below into a shorter version that preserves all requirements, constraints, and intent. \
Output ONLY the condensed text. No preamble. Target: under 150 words.";

/// Compress a long user prompt inline when it exceeds `budget_tokens`.
///
/// Returns `Some(compressed)` only when the prompt exceeds the budget AND
/// compression succeeds. Returns `None` if no compression is needed or fails
/// (caller continues with original prompt — fail open).
pub async fn compress_input_inline(
    prompt: &str,
    budget_tokens: u32,
    model: &str,
    provider_url: &str,
    timeout_secs: u64,
) -> Option<String> {
    let prompt_tokens = estimate_tokens(prompt) as u32;
    if prompt_tokens <= budget_tokens {
        return None; // No compression needed
    }

    tracing::debug!(
        prompt_tokens,
        budget_tokens,
        model,
        "compress_input_inline: prompt exceeds budget, compressing"
    );

    let http = reqwest::Client::new();
    let endpoint = format!("{}/api/chat", provider_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": INPUT_COMPRESS_PROMPT },
            { "role": "user",   "content": prompt }
        ],
        "stream": false,
        "options": { "temperature": 0.0, "num_predict": 300 }
    });

    let resp = http
        .post(&endpoint)
        .json(&body)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "compress_input_inline: Ollama returned error");
        return None;
    }

    let json: serde_json::Value = resp.json().await.ok()?;
    let compressed = json["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if compressed.is_empty() {
        return None;
    }

    tracing::debug!(
        original_tokens = prompt_tokens,
        compressed_tokens = estimate_tokens(&compressed),
        "compress_input_inline: done"
    );

    Some(compressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_four_chars_per_token() {
        // "hello" = 5 chars → 1 token (integer division)
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn estimate_tokens_scales_with_length() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }
}
