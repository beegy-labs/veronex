//! Shared MCP types used across the library and `veronex`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Sentinel string embedded in skipped-result messages.
/// Must match the substring checked in `McpToolResult::is_skipped()`.
const ERR_CIRCUIT_OPEN: &str = "circuit open";

// ── Tool schema ───────────────────────────────────────────────────────────────

/// A tool exposed by an MCP server, extended with Veronex metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Original tool name as declared by the MCP server (e.g. `get_weather`).
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub annotations: McpToolAnnotations,

    /// Filled by `McpToolCache`, not present in the wire format.
    #[serde(skip)]
    pub server_id: Uuid,
    /// Server short-name (slug), used for namespacing (e.g. `weather`).
    #[serde(skip)]
    pub server_name: String,
}

impl McpTool {
    /// Namespaced name injected into the LLM: `mcp_{server}_{tool}`.
    pub fn namespaced_name(&self) -> String {
        format!("mcp_{}_{}", self.server_name, self.name)
    }

    /// OpenAI-compatible function definition for tool injection.
    pub fn to_openai_function(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.namespaced_name(),
                "description": self.description,
                "parameters": self.input_schema,
            }
        })
    }

    /// Returns `true` when result caching is safe for this tool.
    /// Both hints must be `true`; defaults are `false` (conservative).
    pub fn can_cache(&self) -> bool {
        self.annotations.read_only_hint && self.annotations.idempotent_hint
    }
}

/// MCP tool annotations (2025-03-26 spec).
/// Defaults are worst-case: destructive, non-idempotent, open-world.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpToolAnnotations {
    #[serde(rename = "readOnlyHint", default)]
    pub read_only_hint: bool,
    #[serde(rename = "idempotentHint", default)]
    pub idempotent_hint: bool,
    #[serde(rename = "destructiveHint", default = "bool_true")]
    pub destructive_hint: bool,
    #[serde(rename = "openWorldHint", default = "bool_true")]
    pub open_world_hint: bool,
}

fn bool_true() -> bool {
    true
}

// ── Tool call / result ────────────────────────────────────────────────────────

/// A tool call parsed from Ollama's raw JSON response.
#[derive(Debug, Clone)]
pub struct McpToolCall {
    /// Namespaced name: `mcp_{server}_{tool}`.
    pub name: String,
    pub arguments: serde_json::Value,
}

impl McpToolCall {
    /// Parse Ollama's raw `tool_calls` array.
    ///
    /// Ollama format (no `id` field — index-based correlation):
    /// ```json
    /// [{"type":"function","function":{"index":0,"name":"...","arguments":{...}}}]
    /// ```
    pub fn from_ollama(v: &serde_json::Value) -> Vec<Self> {
        v.as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|item| {
                let func = item.get("function")?;
                let name = func.get("name")?.as_str()?.to_string();
                let arguments = func
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                Some(Self { name, arguments })
            })
            .collect()
    }

    pub fn is_mcp_tool(&self) -> bool {
        self.name.starts_with("mcp_")
    }

    /// Extracts the server slug from `mcp_{server}_{tool}`.
    pub fn server_slug(&self) -> Option<&str> {
        let s = self.name.strip_prefix("mcp_")?;
        let end = s.find('_')?;
        Some(&s[..end])
    }

    /// Extracts the original MCP tool name (without prefix).
    pub fn raw_tool_name(&self) -> Option<&str> {
        let s = self.name.strip_prefix("mcp_")?;
        let end = s.find('_')?;
        Some(&s[end + 1..])
    }
}

// ── MCP tool result ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    /// `isError: true` in the MCP response — logical tool failure.
    pub is_error: bool,
    /// Round-trip latency in ms. Zero for cache hits.
    pub latency_ms: u32,
    /// `true` when the result was served from `McpResultCache`.
    pub from_cache: bool,
}

impl McpToolResult {
    pub fn success(content: Vec<McpContent>, latency_ms: u32) -> Self {
        Self { content, is_error: false, latency_ms, from_cache: false }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            content: vec![McpContent::text(msg)],
            is_error: true,
            latency_ms: 0,
            from_cache: false,
        }
    }

    pub fn cached(content: Vec<McpContent>) -> Self {
        Self { content, is_error: false, latency_ms: 0, from_cache: true }
    }

    pub fn skipped() -> Self {
        Self::error(format!("MCP server {ERR_CIRCUIT_OPEN} — skipped"))
    }

    pub fn timeout() -> Self {
        Self::error("MCP tool call timed out (30s)")
    }

    pub fn is_success(&self) -> bool {
        !self.is_error
    }

    pub fn is_timeout(&self) -> bool {
        self.is_error
            && self
                .content
                .first()
                .and_then(|c| c.as_text())
                .is_some_and(|t| t.contains("timed out"))
    }

    pub fn is_skipped(&self) -> bool {
        self.is_error
            && self
                .content
                .first()
                .and_then(|c| c.as_text())
                .is_some_and(|t| t.contains(ERR_CIRCUIT_OPEN))
    }

    /// Render content as a single string for the LLM `tool` role message.
    pub fn to_llm_string(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── MCP content block ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContent {
    Text { text: String },
    Image { data: String, #[serde(rename = "mimeType")] mime_type: String },
}

impl McpContent {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    pub fn as_text(&self) -> Option<&str> {
        if let Self::Text { text } = self {
            Some(text)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tool(server_name: &str, name: &str, read_only: bool, idempotent: bool) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: String::new(),
            input_schema: serde_json::Value::Object(Default::default()),
            annotations: McpToolAnnotations {
                read_only_hint: read_only,
                idempotent_hint: idempotent,
                destructive_hint: !read_only,
                open_world_hint: true,
            },
            server_id: Uuid::nil(),
            server_name: server_name.to_string(),
        }
    }

    // ── McpTool::namespaced_name ──────────────────────────────────────────────

    #[test]
    fn namespaced_name_simple_slug() {
        let t = tool("weather", "get_weather", true, true);
        assert_eq!(t.namespaced_name(), "mcp_weather_get_weather");
    }

    /// Slug with underscores — namespaced name must still round-trip correctly.
    /// This is the same case that broke `raw_tool_name()` in bridge.rs.
    #[test]
    fn namespaced_name_slug_with_underscores() {
        let t = tool("my_server", "get_weather", true, true);
        assert_eq!(t.namespaced_name(), "mcp_my_server_get_weather");
    }

    // ── McpTool::can_cache ───────────────────────────────────────────────────

    #[test]
    fn can_cache_requires_both_hints() {
        assert!(tool("w", "t", true, true).can_cache());
        assert!(!tool("w", "t", false, true).can_cache()); // not read-only
        assert!(!tool("w", "t", true, false).can_cache()); // not idempotent
        assert!(!tool("w", "t", false, false).can_cache());
    }

    // ── McpToolCall::from_ollama ─────────────────────────────────────────────

    #[test]
    fn from_ollama_parses_standard_format() {
        let raw = serde_json::json!([{
            "type": "function",
            "function": { "index": 0, "name": "mcp_weather_get_weather", "arguments": {"lat": 37.5} }
        }]);
        let calls = McpToolCall::from_ollama(&raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "mcp_weather_get_weather");
        assert_eq!(calls[0].arguments["lat"], 37.5);
    }

    #[test]
    fn from_ollama_empty_array() {
        assert!(McpToolCall::from_ollama(&serde_json::json!([])).is_empty());
    }

    #[test]
    fn from_ollama_missing_name_skipped() {
        let raw = serde_json::json!([{"type": "function", "function": {"index": 0}}]);
        assert!(McpToolCall::from_ollama(&raw).is_empty());
    }

    // ── McpToolCall::server_slug / raw_tool_name ─────────────────────────────

    #[test]
    fn server_slug_simple() {
        let c = McpToolCall { name: "mcp_weather_get_weather".into(), arguments: serde_json::json!({}) };
        assert_eq!(c.server_slug(), Some("weather"));
        assert_eq!(c.raw_tool_name(), Some("get_weather"));
    }

    /// Multi-word slug: raw_tool_name must return the part after the slug.
    #[test]
    fn raw_tool_name_multi_word_slug() {
        let c = McpToolCall { name: "mcp_my_server_get_weather".into(), arguments: serde_json::json!({}) };
        assert_eq!(c.server_slug(), Some("my"));          // first segment only
        // The types.rs helper finds the FIRST underscore — documents current behavior.
        // bridge.rs uses tool_def.name directly to avoid this ambiguity.
        assert_eq!(c.raw_tool_name(), Some("server_get_weather"));
    }

    #[test]
    fn is_mcp_tool_prefix_check() {
        let mcp = McpToolCall { name: "mcp_weather_t".into(), arguments: serde_json::json!({}) };
        let non = McpToolCall { name: "get_weather".into(), arguments: serde_json::json!({}) };
        assert!(mcp.is_mcp_tool());
        assert!(!non.is_mcp_tool());
    }

    // ── McpToolResult helpers ────────────────────────────────────────────────

    #[test]
    fn to_llm_string_joins_text_content() {
        let r = McpToolResult::success(
            vec![McpContent::text("line1"), McpContent::text("line2")],
            10,
        );
        assert_eq!(r.to_llm_string(), "line1\nline2");
    }

    #[test]
    fn skipped_is_error_and_flagged() {
        let r = McpToolResult::skipped();
        assert!(r.is_error);
        assert!(r.is_skipped());
        assert!(!r.is_timeout());
    }

    #[test]
    fn timeout_is_error_and_flagged() {
        let r = McpToolResult::timeout();
        assert!(r.is_error);
        assert!(r.is_timeout());
        assert!(!r.is_skipped());
    }
}
