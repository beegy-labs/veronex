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
//! ```
//!
//! The schema embeds one `oneOf` branch per available tool with the tool's own
//! `parameters` JSON-Schema for `args`. The model can ONLY emit a tool call
//! that names a real tool with structurally-valid arguments, OR a final answer.

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
weather, time, market data, events), call a tool first. Do NOT respond with \
\"I don't have access to real-time data\" — that statement is FALSE because \
you have these tools.\n\
- On each turn output exactly one JSON object — either a tool call \
(`action=\"tool\"`) or a final answer (`action=\"final\"`).\n\
- Choose `final` only after you have gathered enough information from tools \
(or for trivial questions that genuinely need no lookup, e.g. arithmetic).";

/// Extracted action from the model's JSON response.
#[derive(Debug, Clone, PartialEq)]
pub enum ForcedAction {
    /// Model invoked a tool. Caller dispatches via existing `execute_calls`.
    Tool { name: String, args: Value },
    /// Model finished. Caller emits `answer` as the assistant text.
    Final { answer: String },
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
    }
    Some(json!({"oneOf": branches}))
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
    fn schema_includes_one_branch_per_tool_plus_final() {
        let schema = build_forced_json_schema(&[web_search(), calc()], true).unwrap();
        let one_of = schema.get("oneOf").unwrap().as_array().unwrap();
        // 2 tool branches + 1 final
        assert_eq!(one_of.len(), 3);
        assert_eq!(one_of[0]["properties"]["tool"]["const"], "web_search");
        assert_eq!(one_of[1]["properties"]["tool"]["const"], "calc");
        assert_eq!(one_of[2]["properties"]["action"]["const"], "final");
    }

    /// Round 0: `allow_final=false` removes the terminal branch so the model
    /// has no logit space to emit `{"action":"final",...}` and MUST call a
    /// tool. Defends against weak models that disclaim "I don't have access
    /// to real-time data" before even trying a tool.
    #[test]
    fn schema_without_final_has_only_tool_branches() {
        let schema = build_forced_json_schema(&[web_search(), calc()], false).unwrap();
        let one_of = schema["oneOf"].as_array().unwrap();
        assert_eq!(one_of.len(), 2, "no final branch when allow_final=false");
        assert_eq!(one_of[0]["properties"]["tool"]["const"], "web_search");
        assert_eq!(one_of[1]["properties"]["tool"]["const"], "calc");
        // Confirm no branch advertises action=final
        assert!(one_of.iter().all(|b| b["properties"]["action"]["const"] != "final"));
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
        assert_eq!(one_of.len(), 2); // 1 tool (web_search) + 1 final
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
}
