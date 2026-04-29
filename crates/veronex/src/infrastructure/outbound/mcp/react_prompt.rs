//! ReAct shim — system prompt + tool descriptor renderer.
//!
//! SDD: `.specs/veronex/mcp-react-shim.md` §4 (Tier B).
//!
//! Used by the ReAct path in `bridge.rs::run_loop_react` (Tier D) when the
//! underlying model lacks native `tool_calls` support. Mirrors the vision
//! shim pattern (`inference_helpers.rs::analyze_images_for_context`):
//! gateway-side capability adapter, transparent to the caller.

use serde_json::Value;

/// System prompt that teaches the model the ReAct interaction format.
/// Locked text — changes here are CDD-relevant (affect the parser contract
/// in `react_parser.rs` and live test fixtures).
///
/// `{tool_descriptions}` is the only template variable — replaced with the
/// rendered tool list at runtime via `build_react_system_prompt`.
pub const REACT_SYSTEM_PROMPT_TEMPLATE: &str = "\
You have access to tools. To use a tool, respond in this exact format:

Thought: <your reasoning about which tool to use>
Action: <tool_name>
Action Input: <a valid JSON object with the tool arguments>

The system will run the tool and respond with:

Observation: <tool result>

You can chain multiple Thought/Action/Action Input/Observation cycles.

When you have enough information to answer, respond:

Thought: I now know the answer.
Final Answer: <your answer>

Constraints:
- Use ONE tool per response. Wait for Observation before the next Action.
- Action Input MUST be valid JSON (double-quoted keys/strings, no trailing commas).
- Do not invent tools. Only use tools listed below.
- If a tool fails or you don't know, write `Final Answer: <best-effort response>`.

Available tools:
{tool_descriptions}";

/// Render OpenAI-format tool definitions as a markdown bullet list suitable
/// for embedding in a system prompt body.
///
/// Output shape (one entry per tool):
/// ```text
/// - tool_name: human description.
///   schema: {...JSON parameters...}
/// ```
///
/// Returns an empty string for an empty input — caller should skip the
/// ReAct path in that case (no tools = nothing for the model to invoke).
pub fn render_tool_descriptions(tools: &[Value]) -> String {
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
        let desc = f
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("(no description)");
        // Compact (single-line) JSON for the schema — keeps the prompt tight.
        let params = f
            .get("parameters")
            .map(|p| serde_json::to_string(p).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        out.push_str(&format!("- {name}: {desc}\n  schema: {params}\n"));
    }
    out
}

/// Build the final ReAct system prompt by substituting the rendered tool
/// list into the locked template.
///
/// Returns `None` when `tools` is empty — the caller should not invoke the
/// ReAct path with no tools.
pub fn build_react_system_prompt(tools: &[Value]) -> Option<String> {
    let rendered = render_tool_descriptions(tools);
    if rendered.is_empty() {
        return None;
    }
    Some(REACT_SYSTEM_PROMPT_TEMPLATE.replace("{tool_descriptions}", &rendered))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn web_search_tool() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web for information.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                }
            }
        })
    }

    fn calculator_tool() -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "calculator",
                "description": "Evaluate an arithmetic expression.",
                "parameters": {
                    "type": "object",
                    "properties": { "expression": { "type": "string" } },
                    "required": ["expression"]
                }
            }
        })
    }

    // ── render_tool_descriptions ───────────────────────────────────────────

    #[test]
    fn renders_single_tool() {
        let out = render_tool_descriptions(&[web_search_tool()]);
        assert!(out.contains("- web_search: Search the web"));
        assert!(out.contains("schema: "));
        assert!(out.contains("\"query\""));
    }

    #[test]
    fn renders_multiple_tools_in_order() {
        let out = render_tool_descriptions(&[web_search_tool(), calculator_tool()]);
        let i_search = out.find("web_search").expect("web_search present");
        let i_calc = out.find("calculator").expect("calculator present");
        assert!(i_search < i_calc, "preserve declaration order");
    }

    #[test]
    fn empty_input_returns_empty_string() {
        assert_eq!(render_tool_descriptions(&[]), "");
    }

    #[test]
    fn skips_tool_without_function_block() {
        // Defensive: caller may have a malformed tool definition.
        let bad = json!({"type": "function"});
        let out = render_tool_descriptions(&[bad, web_search_tool()]);
        assert!(out.contains("web_search"));
        assert!(!out.contains("(no description)\n  schema: \"\""));
    }

    #[test]
    fn skips_tool_without_name() {
        let bad = json!({"type": "function", "function": {"description": "x"}});
        let out = render_tool_descriptions(&[bad]);
        assert_eq!(out, "");
    }

    #[test]
    fn missing_description_uses_placeholder() {
        let t = json!({
            "type": "function",
            "function": {
                "name": "no_desc",
                "parameters": {}
            }
        });
        let out = render_tool_descriptions(&[t]);
        assert!(out.contains("- no_desc: (no description)"));
    }

    #[test]
    fn missing_parameters_uses_empty_schema() {
        let t = json!({"type": "function", "function": {"name": "no_params", "description": "x"}});
        let out = render_tool_descriptions(&[t]);
        assert!(out.contains("schema: {}"));
    }

    // ── build_react_system_prompt ──────────────────────────────────────────

    #[test]
    fn build_substitutes_template_variable() {
        let prompt = build_react_system_prompt(&[web_search_tool()]).expect("non-empty");
        // Template variable is replaced
        assert!(!prompt.contains("{tool_descriptions}"));
        // Rendered tool name appears in the body
        assert!(prompt.contains("web_search"));
        // Locked sections are intact
        assert!(prompt.contains("Action: <tool_name>"));
        assert!(prompt.contains("Final Answer: <your answer>"));
    }

    #[test]
    fn build_returns_none_for_empty_tools() {
        assert!(build_react_system_prompt(&[]).is_none());
    }

    #[test]
    fn build_returns_none_when_all_tools_invalid() {
        // Only invalid tools → render_tool_descriptions returns empty →
        // build returns None → caller skips ReAct path.
        let bad = json!({"type": "function"});
        assert!(build_react_system_prompt(&[bad]).is_none());
    }

    #[test]
    fn template_includes_critical_constraints() {
        let prompt = build_react_system_prompt(&[web_search_tool()]).unwrap();
        assert!(prompt.contains("ONE tool per response"));
        assert!(prompt.contains("valid JSON"));
        assert!(prompt.contains("Do not invent tools"));
    }

    // ── golden snapshot ────────────────────────────────────────────────────

    #[test]
    fn golden_two_tool_render() {
        // Locks the exact rendering format. Changes here MUST be intentional —
        // the parser in react_parser.rs (Tier C) depends on the exact prefix
        // characters and indentation.
        let out = render_tool_descriptions(&[web_search_tool(), calculator_tool()]);
        let expected = "\
- web_search: Search the web for information.
  schema: {\"properties\":{\"query\":{\"type\":\"string\"}},\"required\":[\"query\"],\"type\":\"object\"}
- calculator: Evaluate an arithmetic expression.
  schema: {\"properties\":{\"expression\":{\"type\":\"string\"}},\"required\":[\"expression\"],\"type\":\"object\"}
";
        assert_eq!(out, expected);
    }
}
