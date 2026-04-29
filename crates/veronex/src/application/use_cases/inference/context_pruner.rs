//! Context pre-flight pruner — DCP-style trim before LLM submission.
//!
//! SDD: `.specs/veronex/conversation-context-compression.md` §5 (Tier C).
//!
//! Algorithm: drop oldest non-system, non-recent-K messages until the
//! `messages[]` array fits under the budget. Insert a single placeholder
//! summary message indicating how many turns were dropped. Always
//! preserves system prompts and the most recent K messages.
//!
//! DCP invariant (per S17 §5.3): the S3 ConversationRecord is **never**
//! modified by pruning. Trim is in-memory only for this LLM call.

use serde_json::{json, Value};

use super::context_budget::count_messages_tokens;

/// Default K when caller doesn't override — preserves the most recent
/// 5 messages (system + the current turn's user/assistant exchange).
pub const DEFAULT_PRESERVE_RECENT: usize = 5;

/// Placeholder content inserted in the trimmed array to signal omission.
/// Format chosen to be self-explanatory to the model: indicates that
/// earlier context exists but was elided to fit the budget.
const PLACEHOLDER_CONTENT: &str = "[earlier conversation turns omitted to fit context window]";

/// Diagnostic report from a `prune_to_budget` call. Used by callers for
/// metrics + log emission.
#[derive(Debug, Clone, Copy)]
pub struct TrimReport {
    /// Tokens before any trim — original messages array size.
    pub initial: u32,
    /// Tokens in the returned array (always <= initial).
    pub budget_after: u32,
    /// Number of original messages dropped from the array (excludes the
    /// placeholder we may have inserted).
    pub dropped: usize,
}

impl TrimReport {
    fn no_op(count: u32) -> Self {
        Self { initial: count, budget_after: count, dropped: 0 }
    }
    /// True when no actual trimming occurred (returned array == input).
    pub fn is_no_op(&self) -> bool {
        self.dropped == 0
    }
}

fn is_system_message(m: &Value) -> bool {
    m.get("role").and_then(Value::as_str) == Some("system")
}

/// Trim `messages` to fit under `budget` tokens.
///
/// Pure function — no I/O, no provider calls. Caller resolves the budget
/// via `context_lookup` + `context_budget` before calling.
///
/// Algorithm:
/// 1. If `count_messages_tokens(messages) <= budget` → return clone unchanged.
/// 2. Otherwise, scan from the oldest non-system, non-recent-K message and
///    drop it. Re-count after each drop.
/// 3. Once under budget OR no more candidates remain, insert a single
///    placeholder system message at the position of the first dropped one.
/// 4. Return the trimmed array + a `TrimReport`.
///
/// `recent_k` is clamped to a minimum of 1 (always preserve at least the
/// last message — typically the current user query).
pub fn prune_to_budget(
    messages: &[Value],
    budget: u32,
    recent_k: usize,
) -> (Vec<Value>, TrimReport) {
    let initial = count_messages_tokens(messages);
    if initial <= budget {
        return (messages.to_vec(), TrimReport::no_op(initial));
    }

    let n = messages.len();
    let recent_k = recent_k.max(1);
    let recent_start = n.saturating_sub(recent_k);

    let mut keep = vec![true; n];
    let mut dropped: usize = 0;
    let mut first_dropped_at: Option<usize> = None;

    for i in 0..n {
        // Stop scanning once we hit the protected recent-K window.
        if i >= recent_start {
            break;
        }
        // System messages are never dropped.
        if is_system_message(&messages[i]) {
            continue;
        }

        keep[i] = false;
        if first_dropped_at.is_none() {
            first_dropped_at = Some(i);
        }
        dropped += 1;

        // Probe: would the current set of kept messages fit?
        let kept: Vec<Value> = messages
            .iter()
            .enumerate()
            .filter(|(j, _)| keep[*j])
            .map(|(_, m)| m.clone())
            .collect();
        if count_messages_tokens(&kept) <= budget {
            break;
        }
    }

    let mut result: Vec<Value> = messages
        .iter()
        .enumerate()
        .filter(|(j, _)| keep[*j])
        .map(|(_, m)| m.clone())
        .collect();

    // Insert one placeholder if anything was dropped. Position: at the
    // earliest non-system slot in `result` — preserves any leading
    // system messages, then announces the omission.
    if dropped > 0 {
        let placeholder = json!({
            "role": "system",
            "content": PLACEHOLDER_CONTENT,
        });
        let insert_at = result
            .iter()
            .position(|m| !is_system_message(m))
            .unwrap_or(result.len());
        result.insert(insert_at, placeholder);
    }

    let budget_after = count_messages_tokens(&result);
    (
        result,
        TrimReport {
            initial,
            budget_after,
            dropped,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(content: &str) -> Value {
        json!({"role": "user", "content": content})
    }
    fn assistant(content: &str) -> Value {
        json!({"role": "assistant", "content": content})
    }
    fn system(content: &str) -> Value {
        json!({"role": "system", "content": content})
    }

    /// Build a synthetic conversation: 1 system + N (user, assistant) pairs.
    /// Each non-system message has `chunk` repeated in content for size.
    fn synth_convo(pairs: usize, chunk: &str) -> Vec<Value> {
        let mut v = vec![system("You are a helpful assistant.")];
        for i in 0..pairs {
            v.push(user(&format!("Q{i}: {chunk}")));
            v.push(assistant(&format!("A{i}: {chunk}")));
        }
        v
    }

    #[test]
    fn under_budget_returns_unchanged() {
        let convo = synth_convo(2, "short");
        let initial_tokens = count_messages_tokens(&convo);
        let (out, report) = prune_to_budget(&convo, 10_000, 5);
        assert_eq!(out.len(), convo.len());
        assert_eq!(report.dropped, 0);
        assert!(report.is_no_op());
        assert_eq!(report.initial, initial_tokens);
    }

    #[test]
    fn over_budget_drops_oldest_first() {
        // 30 turns × ~100 token chunks → far over 200 token budget
        let convo = synth_convo(30, &"word ".repeat(50));
        let (out, report) = prune_to_budget(&convo, 200, 3);
        assert!(report.dropped > 0);
        assert!(
            report.budget_after <= 200 + 50,  // small slack for placeholder
            "budget_after={} budget=200",
            report.budget_after
        );
        // System prompt must remain
        assert!(out.iter().any(|m| is_system_message(m)
            && m["content"].as_str().unwrap().contains("helpful")));
    }

    #[test]
    fn preserves_system_messages() {
        let convo = synth_convo(20, &"hi ".repeat(100));
        let (out, report) = prune_to_budget(&convo, 100, 3);
        // Original system prompt must survive
        assert!(out.iter().any(|m| {
            is_system_message(m) && m["content"].as_str().unwrap().contains("helpful")
        }));
        // And we definitely dropped something
        assert!(report.dropped > 0);
    }

    #[test]
    fn preserves_last_k_messages_verbatim() {
        let convo = synth_convo(20, &"big ".repeat(80));
        let recent_k = 5;
        let (out, _) = prune_to_budget(&convo, 100, recent_k);
        // Verify the LAST recent_k messages of the input survive verbatim
        let n = convo.len();
        for tail in 0..recent_k {
            let original = &convo[n - 1 - tail];
            // Find this message in the output
            let appears = out.iter().any(|m| m == original);
            assert!(
                appears,
                "message {} (tail+{}) missing from trimmed output",
                n - 1 - tail,
                tail
            );
        }
    }

    #[test]
    fn placeholder_inserted_when_dropped() {
        let convo = synth_convo(20, &"big ".repeat(60));
        let (out, _) = prune_to_budget(&convo, 100, 3);
        let has_placeholder = out
            .iter()
            .any(|m| m["content"].as_str().unwrap_or("").contains("omitted"));
        assert!(has_placeholder);
    }

    #[test]
    fn placeholder_inserted_at_most_once() {
        let convo = synth_convo(50, &"big ".repeat(80));
        let (out, _) = prune_to_budget(&convo, 100, 3);
        let placeholder_count = out
            .iter()
            .filter(|m| m["content"].as_str().unwrap_or("").contains("omitted"))
            .count();
        assert_eq!(placeholder_count, 1);
    }

    #[test]
    fn recent_k_clamped_to_one() {
        // recent_k=0 is a misconfiguration — clamp to 1 so we always keep
        // the last message (typically the current user query).
        let convo = synth_convo(10, &"big ".repeat(80));
        let (out, _) = prune_to_budget(&convo, 50, 0);
        let last_original = convo.last().unwrap();
        assert!(out.iter().any(|m| m == last_original));
    }

    #[test]
    fn empty_messages_returns_empty() {
        let (out, report) = prune_to_budget(&[], 1000, 5);
        assert!(out.is_empty());
        assert!(report.is_no_op());
    }

    #[test]
    fn realistic_30_turn_loop_fits_target_budget() {
        // 30-round MCP loop: each round = user + assistant ~250 tokens
        let convo = synth_convo(30, &"data point ".repeat(40));
        let initial = count_messages_tokens(&convo);
        // Target: fit in 5K budget (representing 8K-ctx model with 0.6 ratio)
        let (_, report) = prune_to_budget(&convo, 5000, 5);
        assert!(report.budget_after <= 5000 + 100);  // tiny slack for placeholder
        assert!(report.initial == initial);
        assert!(report.dropped > 0);
    }

    #[test]
    fn cannot_trim_when_only_system_and_recent_remain() {
        // Edge case: 1 system + 3 recent messages, all already too big.
        // Pruner can't drop further; returns whatever it has + report shows
        // dropped == 0 (we exit the candidate scan with no action).
        let convo = vec![
            system("System prompt is HUGE: long content here ".repeat(20).trim_end().to_string().as_str()),
            user("recent 1"),
            assistant("recent 2"),
            user("recent 3"),
        ];
        let (out, report) = prune_to_budget(&convo, 5, 3);
        // Nothing was dropable (system + recent_k=3 = 4 messages, n=4)
        assert_eq!(report.dropped, 0);
        assert_eq!(out.len(), convo.len());
    }
}
