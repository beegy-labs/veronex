use crate::application::ports::outbound::lab_settings_repository::LabSettings;
use crate::application::ports::outbound::message_store::ConversationRecord;

// ── Eligibility error ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum MultiTurnError {
    ModelTooSmall { actual: f32, required: i32 },
    ContextTooSmall { actual: u32, required: i32 },
    ModelNotAllowed { model: String },
}

impl std::fmt::Display for MultiTurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MultiTurnError::ModelTooSmall { actual, required } =>
                write!(f, "multi-turn conversation requires a {required}B+ model (this model: {actual}B)"),
            MultiTurnError::ContextTooSmall { actual, required } =>
                write!(f, "multi-turn conversation requires {required}+ context window (this model: {actual} tokens)"),
            MultiTurnError::ModelNotAllowed { model } =>
                write!(f, "model '{model}' is not in the multi-turn allowlist"),
        }
    }
}

impl MultiTurnError {
    pub fn code(&self) -> &'static str {
        match self {
            MultiTurnError::ModelTooSmall { .. }    => "model_too_small",
            MultiTurnError::ContextTooSmall { .. }  => "context_too_small",
            MultiTurnError::ModelNotAllowed { .. }  => "model_not_allowed",
        }
    }
}

// ── Model param parsing ────────────────────────────────────────────────────────

/// Parse model parameter count in billions from a model name or tag.
///
/// Matches patterns like `"7b"`, `"13b"`, `"0.5b"` anywhere in the name.
/// Requires the digit sequence to be preceded by a non-alpha character (or start)
/// to avoid matching unit suffixes like `"mb"` or `"gb"`.
///
/// Returns `None` when no param count is found — **fail open** (unknown → allow).
pub fn model_param_billions(model: &str) -> Option<f32> {
    let s = model.to_lowercase();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Only start matching on a digit or '.'
        if bytes[i].is_ascii_digit() || bytes[i] == b'.' {
            // Check: not preceded by a letter
            let preceded_by_letter = i > 0 && bytes[i - 1].is_ascii_alphabetic();
            if !preceded_by_letter {
                // Collect number string
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                // Must be followed immediately by 'b'
                if i < bytes.len() && bytes[i] == b'b' {
                    // Not followed by another letter (avoid "base", "bits", etc.)
                    let followed_by_letter = i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic();
                    if !followed_by_letter {
                        if let Ok(n) = s[start..i].parse::<f32>() {
                            if n > 0.0 {
                                return Some(n);
                            }
                        }
                    }
                }
                continue;
            }
        }
        i += 1;
    }
    None
}

// ── Eligibility check ─────────────────────────────────────────────────────────

/// Check if a model is eligible for multi-turn conversation.
///
/// Fail-open: `None` values (unknown param count / ctx) pass all checks.
pub fn check_multiturn_eligibility(
    model_name: &str,
    max_ctx: Option<u32>,
    lab: &LabSettings,
) -> Result<(), MultiTurnError> {
    // 1. Model parameter count
    if let Some(params) = model_param_billions(model_name) {
        if params < lab.multiturn_min_params as f32 {
            return Err(MultiTurnError::ModelTooSmall {
                actual: params,
                required: lab.multiturn_min_params,
            });
        }
    }

    // 2. Context window
    if let Some(ctx) = max_ctx {
        if ctx < lab.multiturn_min_ctx as u32 {
            return Err(MultiTurnError::ContextTooSmall {
                actual: ctx,
                required: lab.multiturn_min_ctx,
            });
        }
    }

    // 3. Allowlist (empty = all models allowed)
    if !lab.multiturn_allowed_models.is_empty()
        && !lab.multiturn_allowed_models.iter().any(|m| m == model_name)
    {
        return Err(MultiTurnError::ModelNotAllowed {
            model: model_name.to_string(),
        });
    }

    Ok(())
}

// ── Context assembly ──────────────────────────────────────────────────────────

/// Assemble history messages for Turn N+1 from a `ConversationRecord`.
///
/// Returns Ollama-format messages (`[{"role": "...", "content": "..."}]`).
///
/// Strategy:
/// - Last `recent_verbatim_window` turns → raw prompt + result (verbatim).
/// - Earlier turns → compressed summary if available; raw fallback otherwise.
/// - Budget enforcement: drops oldest messages when total tokens exceed limit.
///
/// `configured_ctx`: real Ollama context window (from Valkey cache).
/// Falls back to `32_768` if unknown.
pub fn assemble(
    record: &ConversationRecord,
    configured_ctx: u32,
    lab: &LabSettings,
) -> Vec<serde_json::Value> {
    let budget = (configured_ctx as f32 * lab.context_budget_ratio) as usize;
    let turns: Vec<_> = record.regular_turns().collect();
    let n = turns.len();
    let verbatim_start = n.saturating_sub(lab.recent_verbatim_window as usize);

    let mut messages: Vec<serde_json::Value> = Vec::with_capacity(n * 2);

    for (i, turn) in turns.iter().enumerate() {
        if i < verbatim_start {
            // Prefer compressed summary; use raw if compression not yet available
            if let Some(ref c) = turn.compressed {
                messages.push(serde_json::json!({ "role": "user", "content": c.summary }));
            } else {
                messages.push(serde_json::json!({ "role": "user", "content": turn.prompt }));
                if let Some(ref result) = turn.result {
                    messages.push(serde_json::json!({ "role": "assistant", "content": result }));
                }
            }
        } else {
            // Verbatim window: always use raw
            messages.push(serde_json::json!({ "role": "user", "content": turn.prompt }));
            if let Some(ref result) = turn.result {
                messages.push(serde_json::json!({ "role": "assistant", "content": result }));
            }
        }
    }

    enforce_budget(&mut messages, budget);
    messages
}

// ── Budget enforcement ────────────────────────────────────────────────────────

fn estimate_tokens_msgs(messages: &[serde_json::Value]) -> usize {
    messages
        .iter()
        .filter_map(|m| m["content"].as_str())
        .map(|s| s.len() / 4)
        .sum()
}

/// Drop oldest messages until the total estimated token count is within budget.
/// Always keeps at least the last message (current input guard handled by caller).
fn enforce_budget(messages: &mut Vec<serde_json::Value>, budget: usize) {
    while estimate_tokens_msgs(messages) > budget && messages.len() > 1 {
        messages.remove(0);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_parsing() {
        assert_eq!(model_param_billions("llama3.2:7b"), Some(7.0));
        assert_eq!(model_param_billions("qwen2.5:3b"), Some(3.0));
        assert_eq!(model_param_billions("qwen2.5:0.5b"), Some(0.5));
        assert_eq!(model_param_billions("mistral:13b"), Some(13.0));
        assert_eq!(model_param_billions("llama3.1:70b"), Some(70.0));
        assert_eq!(model_param_billions("mistral:latest"), None);
        assert_eq!(model_param_billions("noparams"), None);
        // Should not match unit suffixes
        assert_eq!(model_param_billions("some-mb-model"), None);
    }

    #[test]
    fn eligibility_pass() {
        let lab = LabSettings { multiturn_min_params: 7, multiturn_min_ctx: 16384, multiturn_allowed_models: vec![], ..Default::default() };
        assert!(check_multiturn_eligibility("llama3:7b", Some(16384), &lab).is_ok());
        // Fail open: unknown params
        assert!(check_multiturn_eligibility("mistral:latest", Some(16384), &lab).is_ok());
        // Fail open: unknown ctx
        assert!(check_multiturn_eligibility("llama3:7b", None, &lab).is_ok());
    }

    #[test]
    fn eligibility_too_small() {
        let lab = LabSettings { multiturn_min_params: 7, multiturn_min_ctx: 16384, multiturn_allowed_models: vec![], ..Default::default() };
        assert!(check_multiturn_eligibility("qwen2.5:3b", Some(16384), &lab).is_err());
        assert!(check_multiturn_eligibility("llama3:7b", Some(8192), &lab).is_err());
    }

    #[test]
    fn eligibility_allowlist() {
        let lab = LabSettings {
            multiturn_min_params: 7,
            multiturn_min_ctx: 16384,
            multiturn_allowed_models: vec!["qwen2.5:7b".to_string()],
            ..Default::default()
        };
        assert!(check_multiturn_eligibility("qwen2.5:7b", Some(16384), &lab).is_ok());
        assert!(check_multiturn_eligibility("llama3:7b", Some(16384), &lab).is_err());
    }
}
