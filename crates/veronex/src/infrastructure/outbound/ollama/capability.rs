//! Per-model capability detection for Ollama-served models.
//!
//! SDD: `.specs/veronex/mcp-react-shim.md` §3 (Tier A).
//!
//! Sources truth from Ollama's own `/api/show` template — the same signal
//! Ollama uses internally to validate the `tools` request field. Avoids a
//! hand-rolled name-pattern heuristic which is brittle against new model
//! conventions and fine-tune names.
//!
//! Heuristic fallback applies when `/api/show` is unreachable; conservative
//! by design — prefers "no native tool calls" on uncertainty so the ReAct
//! shim activates.

use std::time::Duration;

use serde_json::Value;

const CAPABILITY_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-model capability flags inferred from `/api/show` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OllamaCapability {
    /// Model's chat template renders the `{{ if .Tools }}` block — Ollama
    /// will inject tool schemas natively when `tools` is in the request.
    pub supports_native_tool_calls: bool,
    /// Model's template references `Images` — supports vision input directly
    /// (no need for the gateway's `analyze_images_for_context` shim).
    pub supports_vision: bool,
    /// Native context window per `model_info.*context_length`. 0 when
    /// undeterminable; callers should fall back to
    /// `application::use_cases::inference::context_lookup` (S17 Tier A).
    pub configured_ctx: u32,
}

impl OllamaCapability {
    /// Conservative default: no native tool calls, no vision, unknown ctx.
    /// Returned by callers when both /api/show and the heuristic decline.
    pub const CONSERVATIVE: Self = Self {
        supports_native_tool_calls: false,
        supports_vision: false,
        configured_ctx: 0,
    };
}

/// Query Ollama's `/api/show` and parse capability flags from the template +
/// model_info. Returns `None` on transport / parse failure — caller falls
/// back to `heuristic_supports_native`.
pub async fn fetch_capability(
    http: &reqwest::Client,
    provider_url: &str,
    model: &str,
) -> Option<OllamaCapability> {
    let endpoint = format!("{}/api/show", provider_url.trim_end_matches('/'));
    let body = serde_json::json!({ "name": model });
    let resp = http
        .post(&endpoint)
        .json(&body)
        .timeout(CAPABILITY_TIMEOUT)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: Value = resp.json().await.ok()?;
    Some(parse_capability_json(&json))
}

/// Public for unit tests — extracts flags from a parsed `/api/show` body.
pub fn parse_capability_json(json: &Value) -> OllamaCapability {
    let template = json
        .get("template")
        .and_then(Value::as_str)
        .unwrap_or("");
    // Ollama's chat template uses `{{ if .Tools }}` (Go template) when the
    // model is fine-tuned for native function calling. Detection is a
    // substring check against the template body.
    let supports_native_tool_calls =
        template.contains(".Tools") || template.contains("{{- if .Tools");
    // Vision-capable templates reference the `.Images` (or `Images`) field.
    let supports_vision =
        template.contains(".Images") || template.contains("{{- if .Images");
    // Architecture-prefixed: `llama.context_length`, `qwen3.context_length`,
    // `gemma.context_length`, etc. Try the generic key as a fallback.
    let configured_ctx = json
        .get("model_info")
        .and_then(|info| {
            info.as_object().and_then(|m| {
                m.iter()
                    .find(|(k, _)| k.ends_with(".context_length"))
                    .and_then(|(_, v)| v.as_u64())
                    .or_else(|| m.get("context_length").and_then(Value::as_u64))
            })
        })
        .unwrap_or(0) as u32;
    OllamaCapability {
        supports_native_tool_calls,
        supports_vision,
        configured_ctx,
    }
}

/// Name-pattern heuristic — last-resort fallback when `/api/show` is
/// unreachable (provider down, network error). **Conservative**: only
/// returns `true` for model families known to natively support tool_calls
/// per upstream documentation as of 2026-04. Everything else returns
/// `false`, which routes to the ReAct shim.
///
/// Sources:
/// - Qwen team docs (qwen3-coder series + qwen2.5-instruct/coder)
/// - Llama 3.1+ release notes
/// - Mistral instruct v0.3+
/// - Gemma 4 (Apr 2026 release — native function calling)
pub fn heuristic_supports_native(model: &str) -> bool {
    let m = model.to_lowercase();
    // Qwen3-Coder variants (large context, native tool_calls reliable)
    if m.starts_with("qwen3-coder") {
        return true;
    }
    // Qwen2.5 instruct + coder
    if m.contains("qwen2.5-instruct") || m.contains("qwen2.5-coder") {
        return true;
    }
    // Llama 3.1 / 3.2 / 3.3 (instruct variants — base 3.0 lacks native tools)
    if m.starts_with("llama3.1")
        || m.starts_with("llama-3.1")
        || m.starts_with("llama3.2")
        || m.starts_with("llama-3.2")
        || m.starts_with("llama3.3")
        || m.starts_with("llama-3.3")
    {
        return true;
    }
    // Mistral instruct v0.3+ (older v0.1/v0.2 lack reliable tools)
    if m.contains("mistral") && m.contains("instruct") && m.contains("v0.3") {
        return true;
    }
    // Hermes / Nous fine-tunes (function-calling-trained) + Cohere Command-R
    if m.starts_with("hermes-") || m.starts_with("nous-") || m.starts_with("command-r") {
        return true;
    }
    // Gemma 4 (Apr 2026 — native function calling per release notes)
    if m.contains("gemma4") || m.contains("gemma-4") {
        return true;
    }
    // Default: route to ReAct shim
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── /api/show parse ────────────────────────────────────────────────────

    #[test]
    fn parses_native_tool_template() {
        let body = json!({
            "template": "{{- if .Tools }}You have access to tools{{- end }}",
            "model_info": { "qwen3.context_length": 262_144 }
        });
        let cap = parse_capability_json(&body);
        assert!(cap.supports_native_tool_calls);
        assert_eq!(cap.configured_ctx, 262_144);
    }

    #[test]
    fn parses_no_tool_template() {
        let body = json!({
            "template": "<|user|>\n{{ .Prompt }}\n<|assistant|>",
            "model_info": { "llama.context_length": 8192 }
        });
        let cap = parse_capability_json(&body);
        assert!(!cap.supports_native_tool_calls);
        assert_eq!(cap.configured_ctx, 8192);
    }

    #[test]
    fn parses_vision_template() {
        let body = json!({
            "template": "{{- if .Images }}{{ .Images }}{{- end }}",
            "model_info": { "gemma.context_length": 32_768 }
        });
        let cap = parse_capability_json(&body);
        assert!(cap.supports_vision);
        assert!(!cap.supports_native_tool_calls);
    }

    #[test]
    fn parses_combined_template() {
        let body = json!({
            "template": "{{- if .Tools }}T{{- end }}{{- if .Images }}I{{- end }}",
            "model_info": { "qwen3.context_length": 131_072 }
        });
        let cap = parse_capability_json(&body);
        assert!(cap.supports_native_tool_calls);
        assert!(cap.supports_vision);
        assert_eq!(cap.configured_ctx, 131_072);
    }

    #[test]
    fn falls_back_when_template_missing() {
        let body = json!({ "model_info": {} });
        let cap = parse_capability_json(&body);
        assert!(!cap.supports_native_tool_calls);
        assert!(!cap.supports_vision);
        assert_eq!(cap.configured_ctx, 0);
    }

    #[test]
    fn handles_generic_context_length() {
        // Some Ollama versions emit `context_length` without architecture prefix
        let body = json!({
            "template": "{{- if .Tools }}{{- end }}",
            "model_info": { "context_length": 16_384 }
        });
        let cap = parse_capability_json(&body);
        assert_eq!(cap.configured_ctx, 16_384);
    }

    // ── heuristic ──────────────────────────────────────────────────────────

    #[test]
    fn heuristic_routes_qwen3_coder_to_native() {
        assert!(heuristic_supports_native("qwen3-coder-next-200k:latest"));
        assert!(heuristic_supports_native("qwen3-coder:7b"));
    }

    #[test]
    fn heuristic_routes_llama_3_1_plus_to_native() {
        assert!(heuristic_supports_native("llama3.1:8b"));
        assert!(heuristic_supports_native("llama3.2:3b-instruct"));
        assert!(heuristic_supports_native("llama-3.3:70b"));
    }

    #[test]
    fn heuristic_routes_unknown_to_react_shim() {
        // The whole point — qwen3:8b (small Qwen3 base, NOT qwen3-coder)
        // is not in the allow list → ReAct path.
        assert!(!heuristic_supports_native("qwen3:8b"));
        assert!(!heuristic_supports_native("llama3:7b"));  // 3.0, no version suffix
        assert!(!heuristic_supports_native("mistral:7b-instruct-v0.2"));  // v0.2 not v0.3
        assert!(!heuristic_supports_native("random-community-finetune:13b"));
    }

    #[test]
    fn heuristic_routes_gemma4_to_native() {
        // Apr 2026 release — native function calling
        assert!(heuristic_supports_native("gemma4:9b"));
        assert!(heuristic_supports_native("gemma-4:27b"));
    }

    #[test]
    fn conservative_constant_is_no_capabilities() {
        assert!(!OllamaCapability::CONSERVATIVE.supports_native_tool_calls);
        assert!(!OllamaCapability::CONSERVATIVE.supports_vision);
        assert_eq!(OllamaCapability::CONSERVATIVE.configured_ctx, 0);
    }
}
