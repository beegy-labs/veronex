//! Forced-JSON gateway shim for MCP on models without native `tool_calls`.
//!
//! Replaces the text-template ReAct shim (`react_prompt.rs` + `react_parser.rs`)
//! with deterministic constrained decoding via Ollama's `format` parameter
//! (GBNF grammar under the hood, GA since Ollama v0.5).
//!
//! # Why not ReAct text?
//!
//! The ReAct shim relies on the model voluntarily following a `Thought:` /
//! `Action:` / `Action Input:` template. Strong instruction-tuned models do;
//! weak / older models (qwen3:8b, llama3:7b, mistral:7b-instruct-v0.2)
//! frequently emit prose instead, producing zero tool calls. That violated the
//! gateway promise:
//!
//! > "feature-richness depends on the gateway's shims, not on each underlying
//! >  model's intrinsic capabilities" — `docs/llm/inference/lab-features.md`
//!
//! With `format: <json_schema>` Ollama's llama.cpp backend masks logits at every
//! decoding step so the only valid continuations are tokens consistent with the
//! schema. The model **cannot** emit non-JSON or invalid JSON — making MCP tool
//! invocation deterministic across every supported Ollama model, regardless of
//! tool-calling fine-tuning.
//!
//! # Schema shape
//!
//! Each round's output is one of:
//!
//! ```json
//! {"action":"tool","tool":"<exact tool name>","args":{...tool's args...}}
//! {"action":"final","answer":"..."}
//! {"action":"refuse","reason":"..."}
//! ```
//!
//! The schema embeds one `oneOf` branch per available tool with the tool's own
//! `parameters` JSON-Schema for `args`. The model can ONLY emit a tool call
//! that names a real tool with structurally-valid arguments, a final answer,
//! or a structured refusal carrying a reason (when no available tool can
//! satisfy the question). All three terminal branches are gated by
//! `allow_final` — round 0 (no tool result yet) blocks them so the model is
//! logit-masked into trying a tool first. SDD:
//! `.specs/veronex/mcp-constrained-decoding-unification.md` §3.5.

use serde_json::{json, Value};

/// System prompt for the forced-JSON shim. Pushes tool-first behaviour to
/// counter weaker models' training-cutoff disclaimers ("I don't have access
/// to real-time data"). The schema does the bulk of enforcement via
/// constrained decoding; this prompt is the semantic guide.
pub const FORCED_JSON_SYSTEM_PROMPT: &str = "\
You are an agent with access to tools. The available tools include \
real-time web search, current weather, and current datetime — they DO have \
access to live, up-to-the-minute data. \n\
- For ANY question about current/recent/today's information (prices, news, \
weather, time, market data, events), call a tool first. Do NOT claim \
\"I don't have access to real-time data\" or \"web search is disabled\" — \
those statements are FALSE because you have these tools.\n\
- On each turn output exactly one JSON object — either a tool call \
(`action=\"tool\"`), a final answer (`action=\"final\"`), or a refusal \
(`action=\"refuse\"`).\n\
- Choose `final` only after you have gathered enough information from tools \
(or for trivial questions that genuinely need no lookup, e.g. arithmetic). \
The `final.answer` MUST cite or use the tool results that have been \
collected — do not produce an answer that contradicts or ignores them.\n\
- Choose `refuse` ONLY when none of the available tools can answer the \
question (e.g. user asks about their personal history with no \
personal-data tool available). Provide a concise reason naming what \
data would have been needed.";

/// Extracted action from the model's JSON response.
#[derive(Debug, Clone, PartialEq)]
pub enum ForcedAction {
    /// Model invoked a tool. Caller dispatches via existing `execute_calls`.
    Tool { name: String, args: Value },
    /// Model finished. Caller emits `answer` as the assistant text.
    Final { answer: String },
    /// Model decided no available tool can satisfy the question. Caller
    /// emits `reason` (with a known sentinel prefix) as the assistant text.
    /// Distinct from `Final` so the audit trail and UI can render refusals
    /// differently from synthesised answers.
    Refuse { reason: String },
}

/// Build the forced-JSON oneOf schema from a list of OpenAI-format tool
/// definitions.
///
/// Returns `None` when `tools` is empty (caller should not invoke the
/// forced-JSON path with no tools — it would degenerate to a `final`-only
/// schema, which is just plain text generation).
///
/// `allow_final` gates whether the terminal `{"action":"final","answer"}`
/// branch is part of the schema. Caller passes `false` on the first round
/// (forces the model to call a tool — defends against weak models that
/// would otherwise emit "I don't have access to real-time data") and
/// `true` once at least one tool result is in context.
///
/// Schema layout (one branch per tool, plus a terminal branch when
/// `allow_final`):
///
/// ```json
/// {
///   "oneOf": [
///     {"type":"object","properties":{
///        "action":{"const":"tool"},
///        "tool":{"const":"<tool_name>"},
///        "args":<tool.parameters>
///     },"required":["action","tool","args"],"additionalProperties":false},
///     ...
///     {"type":"object","properties":{
///        "action":{"const":"final"},
///        "answer":{"type":"string"}
///     },"required":["action","answer"],"additionalProperties":false}
///   ]
/// }
/// ```
pub fn build_forced_json_schema(tools: &[Value], allow_final: bool) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }
    let mut branches: Vec<Value> = Vec::with_capacity(tools.len() + 1);
    for t in tools {
        let f = match t.get("function") {
            Some(f) => f,
            None => continue,
        };
        let name = match f.get("name").and_then(Value::as_str) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let mut args_schema = f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"}));
        if !args_schema.is_object() {
            args_schema = json!({"type": "object"});
        }
        branches.push(json!({
            "type": "object",
            "properties": {
                "action": {"const": "tool"},
                "tool": {"const": name},
                "args": args_schema,
            },
            "required": ["action", "tool", "args"],
            "additionalProperties": false,
        }));
    }
    if branches.is_empty() {
        return None;
    }
    if allow_final {
        branches.push(json!({
            "type": "object",
            "properties": {
                "action": {"const": "final"},
                "answer": {"type": "string"},
            },
            "required": ["action", "answer"],
            "additionalProperties": false,
        }));
        branches.push(json!({
            "type": "object",
            "properties": {
                "action": {"const": "refuse"},
                "reason": {"type": "string", "minLength": 8},
            },
            "required": ["action", "reason"],
            "additionalProperties": false,
        }));
    }
    Some(json!({"oneOf": branches}))
}

/// Sentinel prefix prepended to a `Refuse` reason when emitted as assistant
/// content, so UI and audit consumers can distinguish refusals from final
/// answers without re-parsing.
pub const REFUSAL_PREFIX: &str = "REFUSED: ";

/// Decide whether the schema should expose terminator branches (`final`,
/// `refuse`) for the upcoming round. Round 0 (no tool result yet) returns
/// false to logit-mask the model into picking a tool branch — this is the
/// gateway's defence against weak-model "I don't have access to real-time
/// data" disclaimers emitted before any tool is even tried.
///
/// Codified as a named function (rather than an inline expression) so the
/// invariant is testable and traceable. SDD:
/// `.specs/veronex/mcp-constrained-decoding-unification.md` §3.6.
pub fn allow_final_for_round(prior_tool_calls: usize) -> bool {
    prior_tool_calls > 0
}

/// Wrap a forced-JSON schema in the OpenAI `response_format` envelope so the
/// existing Ollama adapter (`adapter.rs:594`) routes it through to Ollama's
/// `format` field.
pub fn schema_to_response_format(schema: Value) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": "veronex_mcp_action",
            "schema": schema,
        }
    })
}

/// Render a brief, machine-readable tool catalogue to embed in the system
/// prompt body — the schema constrains *form*, this gives the model semantic
/// hints to choose the right tool.
pub fn render_tool_catalogue(tools: &[Value]) -> String {
    let mut out = String::new();
    for t in tools {
        let f = match t.get("function") {
            Some(f) => f,
            None => continue,
        };
        let name = f.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let desc = f.get("description").and_then(Value::as_str).unwrap_or("(no description)");
        out.push_str(&format!("- {name}: {desc}\n"));
    }
    out
}

/// Build the full system prompt: locked instruction + tool catalogue.
pub fn build_forced_json_system_prompt(tools: &[Value]) -> Option<String> {
    let cat = render_tool_catalogue(tools);
    if cat.is_empty() {
        return None;
    }
    Some(format!("{FORCED_JSON_SYSTEM_PROMPT}\n\nAvailable tools:\n{cat}"))
}

/// Parse the model's emitted JSON into a `ForcedAction`.
///
/// The adapter passes `format: <schema>` so output is grammar-constrained — but
/// downstream we still validate defensively (the model could be served by an
/// older Ollama, or the schema could have been bypassed). On parse failure we
/// fall back to treating the entire text as a `final` answer (fail-open: user
/// always sees something).
pub fn parse_forced_action(text: &str) -> ForcedAction {
    let trimmed = text.trim();
    let v: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => {
            return ForcedAction::Final { answer: trimmed.to_string() };
        }
    };
    let action = v.get("action").and_then(Value::as_str).unwrap_or("");
    match action {
        "tool" => {
            let name = v.get("tool").and_then(Value::as_str).unwrap_or("").to_string();
            let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
            if name.is_empty() {
                ForcedAction::Final { answer: trimmed.to_string() }
            } else {
                ForcedAction::Tool { name, args }
            }
        }
        "final" => {
            let answer = v.get("answer").and_then(Value::as_str).unwrap_or("").to_string();
            ForcedAction::Final { answer }
        }
        "refuse" => {
            let reason = v.get("reason").and_then(Value::as_str).unwrap_or("").to_string();
            if reason.is_empty() {
                ForcedAction::Final { answer: trimmed.to_string() }
            } else {
                ForcedAction::Refuse { reason }
            }
        }
        _ => ForcedAction::Final { answer: trimmed.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web_search() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web.",
                "parameters": {
                    "type": "object",
                    "properties": {"query": {"type": "string"}},
                    "required": ["query"]
                }
            }
        })
    }

    fn calc() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "calc",
                "description": "Evaluate.",
                "parameters": {
                    "type": "object",
                    "properties": {"expr": {"type": "string"}},
                    "required": ["expr"]
                }
            }
        })
    }

    #[test]
    fn empty_tools_returns_none() {
        assert!(build_forced_json_schema(&[], true).is_none());
        assert!(build_forced_json_schema(&[], false).is_none());
        assert!(build_forced_json_system_prompt(&[]).is_none());
    }

    #[test]
    fn schema_includes_one_branch_per_tool_plus_terminators() {
        let schema = build_forced_json_schema(&[web_search(), calc()], true).unwrap();
        let one_of = schema.get("oneOf").unwrap().as_array().unwrap();
        // 2 tool branches + 1 final + 1 refuse
        assert_eq!(one_of.len(), 4);
        assert_eq!(one_of[0]["properties"]["tool"]["const"], "web_search");
        assert_eq!(one_of[1]["properties"]["tool"]["const"], "calc");
        assert_eq!(one_of[2]["properties"]["action"]["const"], "final");
        assert_eq!(one_of[3]["properties"]["action"]["const"], "refuse");
    }

    /// Round 0: `allow_final=false` removes both terminator branches so the
    /// model has no logit space to emit `{"action":"final",...}` or
    /// `{"action":"refuse",...}` and MUST call a tool. Defends against weak
    /// models that disclaim "I don't have access to real-time data" before
    /// even trying a tool, AND against premature refusal before the model
    /// has surveyed the available tools.
    #[test]
    fn schema_without_final_has_only_tool_branches() {
        let schema = build_forced_json_schema(&[web_search(), calc()], false).unwrap();
        let one_of = schema["oneOf"].as_array().unwrap();
        assert_eq!(one_of.len(), 2, "no terminator branches when allow_final=false");
        assert_eq!(one_of[0]["properties"]["tool"]["const"], "web_search");
        assert_eq!(one_of[1]["properties"]["tool"]["const"], "calc");
        // Confirm no terminator branches present.
        assert!(one_of.iter().all(|b| {
            let a = &b["properties"]["action"]["const"];
            a != "final" && a != "refuse"
        }));
    }

    #[test]
    fn refuse_branch_requires_reason_with_min_length() {
        let schema = build_forced_json_schema(&[web_search()], true).unwrap();
        let refuse_branch = schema["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["properties"]["action"]["const"] == "refuse")
            .expect("refuse branch present when allow_final=true");
        assert_eq!(refuse_branch["properties"]["reason"]["type"], "string");
        assert_eq!(refuse_branch["properties"]["reason"]["minLength"], 8);
        assert_eq!(refuse_branch["additionalProperties"], false);
    }

    #[test]
    fn allow_final_for_round_blocks_round_zero() {
        assert!(!allow_final_for_round(0), "round 0 must force a tool");
        assert!(allow_final_for_round(1), "after first tool, allow termination");
        assert!(allow_final_for_round(5), "still allowed after many tools");
    }

    #[test]
    fn schema_uses_tool_parameters_for_args() {
        let schema = build_forced_json_schema(&[web_search()], true).unwrap();
        let args_schema = &schema["oneOf"][0]["properties"]["args"];
        assert_eq!(args_schema["properties"]["query"]["type"], "string");
        assert_eq!(args_schema["required"][0], "query");
    }

    #[test]
    fn skips_tool_without_name() {
        let bad = json!({"type": "function", "function": {"description": "x"}});
        let schema = build_forced_json_schema(&[bad, web_search()], true).unwrap();
        let one_of = schema["oneOf"].as_array().unwrap();
        assert_eq!(one_of.len(), 3); // 1 tool (web_search) + final + refuse
        assert_eq!(one_of[0]["properties"]["tool"]["const"], "web_search");
    }

    #[test]
    fn schema_to_response_format_wraps_correctly() {
        let schema = build_forced_json_schema(&[web_search()], true).unwrap();
        let rf = schema_to_response_format(schema.clone());
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["schema"], schema);
    }

    #[test]
    fn parse_tool_action() {
        let action = parse_forced_action(r#"{"action":"tool","tool":"web_search","args":{"query":"micron"}}"#);
        match action {
            ForcedAction::Tool { name, args } => {
                assert_eq!(name, "web_search");
                assert_eq!(args["query"], "micron");
            }
            _ => panic!("expected Tool"),
        }
    }

    #[test]
    fn parse_final_action() {
        let action = parse_forced_action(r#"{"action":"final","answer":"hello"}"#);
        match action {
            ForcedAction::Final { answer } => assert_eq!(answer, "hello"),
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn parse_invalid_json_falls_back_to_final() {
        let action = parse_forced_action("not valid json at all");
        match action {
            ForcedAction::Final { answer } => assert_eq!(answer, "not valid json at all"),
            _ => panic!("expected Final fail-open"),
        }
    }

    #[test]
    fn parse_unknown_action_falls_back_to_final() {
        let raw = r#"{"action":"unknown","payload":42}"#;
        let action = parse_forced_action(raw);
        match action {
            ForcedAction::Final { answer } => assert_eq!(answer, raw),
            _ => panic!("expected Final fail-open"),
        }
    }

    #[test]
    fn parse_refuse_action() {
        let raw = r#"{"action":"refuse","reason":"no personal-history tool available"}"#;
        match parse_forced_action(raw) {
            ForcedAction::Refuse { reason } => {
                assert_eq!(reason, "no personal-history tool available");
            }
            other => panic!("expected Refuse, got {other:?}"),
        }
    }

    #[test]
    fn parse_refuse_with_empty_reason_falls_back_to_final() {
        let raw = r#"{"action":"refuse","reason":""}"#;
        assert!(matches!(parse_forced_action(raw), ForcedAction::Final { .. }));
    }

    #[test]
    fn parse_tool_with_empty_name_falls_back_to_final() {
        let raw = r#"{"action":"tool","tool":"","args":{}}"#;
        let action = parse_forced_action(raw);
        assert!(matches!(action, ForcedAction::Final { .. }));
    }

    #[test]
    fn render_catalogue_lists_all_tools() {
        let cat = render_tool_catalogue(&[web_search(), calc()]);
        assert!(cat.contains("- web_search: Search the web."));
        assert!(cat.contains("- calc: Evaluate."));
    }

    #[test]
    fn system_prompt_includes_catalogue() {
        let prompt = build_forced_json_system_prompt(&[web_search()]).unwrap();
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("Search the web"));
    }

    #[test]
    fn schema_with_no_parameters_uses_object_default() {
        let t = json!({"type": "function", "function": {"name": "noop", "description": "nothing"}});
        let schema = build_forced_json_schema(&[t], true).unwrap();
        // first branch is the tool, args defaulted to {"type": "object"}
        assert_eq!(schema["oneOf"][0]["properties"]["args"]["type"], "object");
    }

    // ── Regression: conv_33AfPaddqdXSiqIHX081T failure modes ──────────────────
    //
    // Live conversation that motivated `mcp-constrained-decoding-unification.md`.
    // All four turns failed the gateway contract under the legacy native path:
    //   Turn 1/3 — model ran 5 tool rounds then emitted disclaimer prose
    //              ("실시간 데이터 접근 권한 없음", "웹검색 비활성화") instead of
    //              synthesising from the 14 KB / 8 KB of tool results in context.
    //   Turn 2/4 — model emitted disclaimer prose at round 0 without trying any tool.
    //
    // These tests pin the *structural* gateway guarantees that prevent each
    // failure mode under the unified constrained-decoding path. Per SDD §5.

    /// Turn 2/4 (round 0 disclaimer) is structurally impossible: with
    /// `allow_final_for_round(0) == false`, the schema has no `final` or
    /// `refuse` branch, so the model's logit space cannot emit a disclaimer.
    /// It MUST select a tool branch.
    #[test]
    fn conv_33af_round_zero_cannot_emit_disclaimer() {
        let prior_tool_calls = 0;
        assert!(
            !allow_final_for_round(prior_tool_calls),
            "round 0 (no tool results) must block terminator branches"
        );
        let tools = vec![web_search()];
        let schema =
            build_forced_json_schema(&tools, allow_final_for_round(prior_tool_calls)).unwrap();
        let one_of = schema["oneOf"].as_array().unwrap();
        // Schema is tool-only — no logit space for "I don't have access".
        assert_eq!(one_of.len(), 1);
        assert_eq!(one_of[0]["properties"]["action"]["const"], "tool");
        // Defensive cross-check: nowhere in the schema is `final` or `refuse`
        // a permitted action constant.
        let serialized = serde_json::to_string(&schema).unwrap();
        assert!(
            !serialized.contains(r#""const":"final""#),
            "round 0 schema MUST NOT advertise final branch: {serialized}"
        );
        assert!(
            !serialized.contains(r#""const":"refuse""#),
            "round 0 schema MUST NOT advertise refuse branch: {serialized}"
        );
    }

    /// Turn 1/3 (post-tool disclaimer) is structurally impossible to encode
    /// as plain text: once tool results exist, the schema allows `final` —
    /// but the model's only path to text is via the `final.answer` string
    /// generated under the GBNF grammar, which is bound to the JSON envelope.
    /// Disclaimer prose CAN appear inside `answer`; this test asserts the
    /// audit trail makes that case auditable: the JSON envelope is
    /// preserved end-to-end so downstream UI can quote-cite the tool
    /// results that the model ignored.
    #[test]
    fn conv_33af_post_tool_disclaimer_is_audit_visible() {
        // After 1 tool call the schema admits final + refuse alongside tools.
        let prior_tool_calls = 1;
        assert!(allow_final_for_round(prior_tool_calls));
        let tools = vec![web_search()];
        let schema =
            build_forced_json_schema(&tools, allow_final_for_round(prior_tool_calls)).unwrap();
        let one_of = schema["oneOf"].as_array().unwrap();
        assert_eq!(one_of.len(), 3, "tool + final + refuse");
        let kinds: Vec<_> = one_of
            .iter()
            .map(|b| b["properties"]["action"]["const"].as_str().unwrap_or(""))
            .collect();
        assert!(kinds.contains(&"tool"));
        assert!(kinds.contains(&"final"));
        assert!(kinds.contains(&"refuse"));

        // Even if the model hides a disclaimer inside `final.answer`, the
        // wrapping JSON is grammar-enforced — the audit pipeline parses it
        // back into a `ForcedAction::Final { answer }` and the assistant
        // bubble shows the answer alongside the tool_calls[] timeline. The
        // failure mode is then *attributable* (model output ignored its own
        // tool results) rather than *invisible* (legacy native path emitted
        // raw prose with no schema, no tag, no audit anchor).
        let answer_with_disclaimer =
            r#"{"action":"final","answer":"실시간 데이터에 접근 권한이 없습니다."}"#;
        match parse_forced_action(answer_with_disclaimer) {
            ForcedAction::Final { answer } => {
                assert!(answer.contains("실시간 데이터"));
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    /// When the model determines no available tool can answer, the schema
    /// forces a structured refusal with reason ≥ 8 chars instead of
    /// silently falling back to disclaimer prose.
    #[test]
    fn conv_33af_refusal_carries_attributable_reason() {
        let raw = r#"{"action":"refuse","reason":"no live market-data tool registered for this ACL"}"#;
        match parse_forced_action(raw) {
            ForcedAction::Refuse { reason } => {
                assert!(reason.len() >= 8);
                assert!(reason.contains("market-data"));
            }
            other => panic!("expected Refuse, got {other:?}"),
        }
    }
}
