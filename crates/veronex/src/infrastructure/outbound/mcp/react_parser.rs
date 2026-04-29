//! ReAct shim — stream-aware Action / Final Answer parser.
//!
//! SDD: `.specs/veronex/mcp-react-shim.md` §5 (Tier C).
//!
//! Consumes a model's text stream chunk-by-chunk and emits structured
//! events: tool calls extracted as `Action { name, args }`, final answer
//! text as `Final(text)`, or pass-through `Text(chunk)` once `Final Answer:`
//! has been seen.
//!
//! Parser is intentionally tolerant: any unparseable trailer at end-of-stream
//! is fail-opened as a `Final` event so the user always sees something.

use serde_json::Value;

const ACTION_MARKER: &str = "Action:";
const ACTION_INPUT_MARKER: &str = "Action Input:";
const FINAL_MARKER: &str = "Final Answer:";

/// Events produced by the parser.
#[derive(Debug, Clone)]
pub enum ReActEvent {
    /// Plain text safe to forward to the SSE client (only emitted after
    /// `Final Answer:` is seen, never inside reasoning blocks).
    Text(String),
    /// A tool invocation extracted from the stream — caller executes via
    /// existing `bridge::execute_calls` machinery and feeds the result back
    /// as an `Observation:` line.
    Action { name: String, args: Value },
    /// Model declared completion; payload is the final answer text.
    Final(String),
    /// End-of-stream sentinel triggered when the buffer has no recognized
    /// pattern and is non-empty — caller forwards as text (fail-open).
    ParseError(String),
}

/// Stream-aware parser. Feed chunks via `feed`; flush trailing data via
/// `finish` at end-of-stream.
pub struct ReActParser {
    buf: String,
    /// Once true, all subsequent chunks are emitted directly as `Text`.
    in_final: bool,
}

impl ReActParser {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            in_final: false,
        }
    }

    /// Feed a chunk of model output. Returns 0+ events extracted from the
    /// accumulated buffer. Caller should drain events as they arrive (Action
    /// triggers tool execution; Final/Text forwards to client).
    pub fn feed(&mut self, chunk: &str) -> Vec<ReActEvent> {
        self.buf.push_str(chunk);
        let mut events = Vec::new();

        loop {
            if self.in_final {
                if self.buf.is_empty() {
                    return events;
                }
                events.push(ReActEvent::Text(std::mem::take(&mut self.buf)));
                return events;
            }

            // Check for Final Answer first — it terminates parsing entirely.
            if let Some(pos) = self.buf.find(FINAL_MARKER) {
                let payload = self.buf[pos + FINAL_MARKER.len()..]
                    .trim_start_matches(|c: char| c == ' ' || c == '\t')
                    .to_string();
                self.buf.clear();
                self.in_final = true;
                if !payload.is_empty() {
                    events.push(ReActEvent::Final(payload));
                }
                continue;
            }

            // Check for Action / Action Input pair
            let action_pos = match self.buf.find(ACTION_MARKER) {
                Some(p) => p,
                None => return events,
            };
            let after_action = &self.buf[action_pos + ACTION_MARKER.len()..];
            let input_rel = match after_action.find(ACTION_INPUT_MARKER) {
                Some(p) => p,
                None => return events, // wait for more chunks
            };
            let name = after_action[..input_rel]
                .trim_matches(|c: char| c == '\n' || c == ' ' || c == '\t' || c == '\r')
                .to_string();
            if name.is_empty() {
                return events; // malformed; wait or fail at finish
            }

            let json_start_in_buf =
                action_pos + ACTION_MARKER.len() + input_rel + ACTION_INPUT_MARKER.len();
            let json_region = &self.buf[json_start_in_buf..];
            let json_offset_in_region = json_region
                .find(|c: char| c == '{' || c == '[')
                .unwrap_or(0);
            let bracket_start_in_buf = json_start_in_buf + json_offset_in_region;
            let bracket_region = &self.buf[bracket_start_in_buf..];

            let end_in_region = match find_balanced_json(bracket_region) {
                Some(e) => e,
                None => return events, // JSON still streaming
            };

            let json_text = &self.buf[bracket_start_in_buf..bracket_start_in_buf + end_in_region];
            let json_text_owned = json_text.to_string();
            let consume_to = bracket_start_in_buf + end_in_region;

            match serde_json::from_str::<Value>(&json_text_owned) {
                Ok(args) => {
                    self.buf.drain(..consume_to);
                    events.push(ReActEvent::Action { name, args });
                    // Continue loop — there may be another Final Answer or
                    // Action after this one (rare but possible).
                    continue;
                }
                Err(e) => {
                    // Bracket-balanced but JSON-invalid — caller fail-opens.
                    let snippet = json_text_owned.chars().take(80).collect::<String>();
                    self.buf.drain(..consume_to);
                    events.push(ReActEvent::ParseError(format!(
                        "invalid Action Input JSON: {} (snippet: {})",
                        e, snippet
                    )));
                    continue;
                }
            }
        }
    }

    /// End-of-stream flush. Anything remaining in the buffer is emitted as
    /// `Text` (in `in_final` mode) or as a fail-open `Final` (otherwise) —
    /// users always see whatever the model produced, even if the format
    /// drifted from the locked template.
    pub fn finish(mut self) -> Vec<ReActEvent> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        if self.in_final {
            return vec![ReActEvent::Text(self.buf)];
        }
        // Fail-open: any non-conforming trailing text is treated as final.
        let trimmed = self.buf.trim().to_string();
        if trimmed.is_empty() {
            return Vec::new();
        }
        vec![ReActEvent::Final(trimmed)]
    }
}

impl Default for ReActParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the byte offset (within `text`) immediately AFTER a balanced JSON
/// object/array. Returns `None` if no balanced structure is found within
/// the input. Handles `"..."` strings (with `\"` escapes) so braces inside
/// strings don't affect depth.
fn find_balanced_json(text: &str) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut started = false;
    let mut byte_pos: usize = 0;
    let mut last_close: usize = 0;

    for c in text.chars() {
        if escape_next {
            escape_next = false;
            byte_pos += c.len_utf8();
            continue;
        }
        if in_string {
            match c {
                '\\' => escape_next = true,
                '"' => in_string = false,
                _ => {}
            }
            byte_pos += c.len_utf8();
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => {
                if !started {
                    started = true;
                }
                depth += 1;
            }
            '}' | ']' => {
                depth -= 1;
                last_close = byte_pos + c.len_utf8();
                if started && depth == 0 {
                    return Some(last_close);
                }
            }
            _ => {}
        }
        byte_pos += c.len_utf8();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(parser: &mut ReActParser, chunks: &[&str]) -> Vec<ReActEvent> {
        let mut all = Vec::new();
        for c in chunks {
            all.extend(parser.feed(c));
        }
        all
    }

    fn extract_action(events: &[ReActEvent]) -> Option<(&str, &Value)> {
        events.iter().find_map(|e| {
            if let ReActEvent::Action { name, args } = e {
                Some((name.as_str(), args))
            } else {
                None
            }
        })
    }

    #[test]
    fn extracts_single_action_one_chunk() {
        let mut p = ReActParser::new();
        let events = collect(
            &mut p,
            &["Thought: search.\nAction: web_search\nAction Input: {\"q\": \"micron\"}\n"],
        );
        let (name, args) = extract_action(&events).unwrap();
        assert_eq!(name, "web_search");
        assert_eq!(args["q"], "micron");
    }

    #[test]
    fn extracts_action_split_across_chunks() {
        let mut p = ReActParser::new();
        let events = collect(
            &mut p,
            &[
                "Thought: I'll search.\nAction: ",
                "web_search\nAction Input:",
                " {\"query\":",
                "\"micron stock\"}\n",
            ],
        );
        let (name, args) = extract_action(&events).unwrap();
        assert_eq!(name, "web_search");
        assert_eq!(args["query"], "micron stock");
    }

    #[test]
    fn handles_braces_inside_string_value() {
        let mut p = ReActParser::new();
        let events = collect(
            &mut p,
            &["Action: web_search\nAction Input: {\"q\": \"a {b} c\"}\n"],
        );
        let (_, args) = extract_action(&events).unwrap();
        assert_eq!(args["q"], "a {b} c");
    }

    #[test]
    fn final_answer_emits_final_event() {
        let mut p = ReActParser::new();
        let events = collect(
            &mut p,
            &["Thought: I now know the answer.\nFinal Answer: 답변..."],
        );
        let final_text = events.iter().find_map(|e| {
            if let ReActEvent::Final(t) = e {
                Some(t.as_str())
            } else {
                None
            }
        });
        assert_eq!(final_text, Some("답변..."));
    }

    #[test]
    fn final_answer_followed_by_text_emits_text_chunks() {
        let mut p = ReActParser::new();
        let mut all = p.feed("Final Answer: hello");
        all.extend(p.feed(" world"));
        // First event is Final("hello"), second is Text(" world")
        let final_count = all.iter().filter(|e| matches!(e, ReActEvent::Final(_))).count();
        let text_count = all.iter().filter(|e| matches!(e, ReActEvent::Text(_))).count();
        assert_eq!(final_count, 1);
        assert_eq!(text_count, 1);
    }

    #[test]
    fn plain_text_no_markers_yields_pending_then_finish_emits_final() {
        let mut p = ReActParser::new();
        let live = p.feed("Just plain text without any keywords");
        // No marker → no events yet (buffering)
        assert!(live.is_empty());
        // At EOS, fail-open as Final
        let trail = p.finish();
        assert!(matches!(trail.first(), Some(ReActEvent::Final(_))));
    }

    #[test]
    fn invalid_json_in_action_input_emits_parse_error() {
        let mut p = ReActParser::new();
        // Bracket-balanced but invalid JSON (single-quoted keys)
        let events = collect(
            &mut p,
            &["Action: web_search\nAction Input: {q: \"missing-quotes\"}\n"],
        );
        assert!(events.iter().any(|e| matches!(e, ReActEvent::ParseError(_))));
    }

    #[test]
    fn truncated_action_input_yields_no_event_until_finish() {
        let mut p = ReActParser::new();
        // JSON is incomplete (no closing brace yet)
        let events = collect(&mut p, &["Action: web_search\nAction Input: {\"q\": \"micron"]);
        assert!(events.iter().all(|e| !matches!(e, ReActEvent::Action { .. })));
        // finish() should fail-open as Final
        let trail = p.finish();
        assert!(matches!(trail.first(), Some(ReActEvent::Final(_))));
    }

    #[test]
    fn final_answer_only_no_text_payload() {
        let mut p = ReActParser::new();
        // "Final Answer:" alone with nothing after → no event yet (buffering for content)
        // then a chunk with content arrives.
        let _ = p.feed("Final Answer:");
        let after = p.feed(" the answer is yes");
        assert!(matches!(after.first(), Some(ReActEvent::Text(_))));
    }

    #[test]
    fn nested_json_in_action_input() {
        let mut p = ReActParser::new();
        let events = collect(
            &mut p,
            &["Action: complex_tool\nAction Input: {\"filter\": {\"q\": \"x\", \"limit\": 5}, \"sort\": [\"-date\"]}\n"],
        );
        let (_, args) = extract_action(&events).unwrap();
        assert_eq!(args["filter"]["q"], "x");
        assert_eq!(args["sort"][0], "-date");
    }

    #[test]
    fn balanced_json_helper_returns_none_for_unbalanced() {
        assert!(find_balanced_json("{\"q\": \"x\"").is_none());
        assert!(find_balanced_json("plain").is_none());
        assert_eq!(find_balanced_json("{}"), Some(2));
        assert_eq!(find_balanced_json("[1, 2, 3] junk"), Some(9));
    }

    #[test]
    fn empty_input_no_events() {
        let mut p = ReActParser::new();
        let events = p.feed("");
        assert!(events.is_empty());
        let trail = p.finish();
        assert!(trail.is_empty());
    }
}
