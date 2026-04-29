# SDD: MCP ReAct Shim (gateway tool-calling fallback for non-native models)

> Status: planned (research complete, implementation ready) | Change type: **Add** | Created: 2026-04-29 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/inference/lab-features.md` · `docs/llm/policies/architecture.md` · `crates/veronex/src/infrastructure/inbound/http/inference_helpers.rs::analyze_images_for_context` (vision shim — pattern reference)
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S18
> ADD framework: `.add/feature-addition.md` (spec-first) · `.add/implementation.md` (hexagonal, 10K providers / 1M TPS scale)
> **Resume rule**: every section is self-contained.

---

## §0 Quick-resume State

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| A — Capability detection (`/api/show` template inspection + cache) | [ ] | `feat/mcp-react-fallback` | — | — |
| B — ReAct prompt template + tool descriptor renderer | [ ] | (same) | — | — |
| C — Stream-aware Action parser (regex + bracket-counting JSON) | [ ] | (same) | — | — |
| D — `bridge.rs::run_loop` path branch (native vs ReAct) + Observation feedback | [ ] | (same) | — | — |
| E — Tests (parser unit + integration on small model) | [ ] | (same) | — | — |
| CDD-sync — `mcp.md` adds shim section | [ ] | — | — | — |
| Live verify (dev) — `qwen3:8b` calls `web_search` via ReAct path | [ ] | — | — | — |

---

## §1 Problem (gateway pattern gap)

### §1.1 Project pattern: gateway shims for missing model capabilities

veronex (per `.ai/README.md`) is a **gateway** that lets ANY underlying Ollama model participate in capabilities the model itself may not natively support. Existing example — vision shim (`inference_helpers.rs:150 analyze_images_for_context`):

```
non-vision model + image → gateway delegates description via vision_model
                       → text description prepended to prompt
                       → non-vision model "supports" images
```

The gateway's promise: feature-richness depends on the gateway's shims, not on each underlying model's intrinsic capabilities.

### §1.2 Missing shim: native tool-calling

MCP integration (S11/S12/S15/S16) implemented end-to-end native OpenAI-style `tool_calls`. Models without native `tool_calls` get **no MCP through this gateway today** — even when capable of agentic behavior via ReAct prompt patterns. The shim pattern is missing for tool-calling.

### §1.3 Concrete failure (verified 2026-04-29)

`conv_337tGMRdShMz35be763bn` (job `019dd816`, model `qwen3:8b`):
- MCP intercept fired, 4 tools attached, 0 native `tool_calls` emitted.
- Same prompt on `qwen3-coder-next-200k:latest` → tool_call emitted natively.
- Failure is per-model (qwen3:8b "decides not to use tools"), not per-format. Industry confirms: small models (≤8B) routinely ignore native tools ([Medium — Ollama Tool support](https://medium.com/@laurentkubaski/ollama-tool-support-aka-function-calling-23a1c0189bee)).

### §1.4 Decision

**Add ReAct shim** mirroring vision shim. Hybrid routing: native path stays default for capable models (Qwen3 large, Llama3.1+, etc.); ReAct path handles the long tail. **Per Qwen team docs, ReAct is NOT recommended for Qwen3 reasoning models** — capability detection (Tier A) handles this routing automatically.

---

## §2 Solution — concrete tier breakdown

### §2.1 Design choices (web-research backed)

| Question | Answer | Source |
|---|---|---|
| How to detect "supports native tool_calls"? | **Query Ollama `/api/show`** and inspect template for `.Tools` variable — the same mechanism Ollama itself uses internally | [Ollama Tool Calling docs](https://docs.ollama.com/capabilities/tool-calling), [DeepWiki — Ollama tool calling](https://deepwiki.com/ollama/ollama/7.2-tool-calling-and-function-execution) |
| ReAct parser style | **`ReActJsonSingleInputOutputParser` pattern** (LangChain): regex extracts `Action: <name>` + `Action Input: <JSON>`, bracket-counting handles multi-line JSON | [LangChain ReActJsonSingleInputOutputParser](https://api.python.langchain.com/en/latest/_modules/langchain/agents/output_parsers/react_json_single_input.html) |
| Stream parsing strategy | **Buffer until block-complete**: detect `Action Input:` line, then count balanced `{}/[]` braces for the JSON; emit when balanced | [LangChain Tutorials — ReAct 2026](https://langchain-tutorials.github.io/langchain-react-agent-pattern-2026/) |
| Failure handling | **`handle_parsing_errors=True`-style fail-open**: unparseable output treated as Final Answer; explicit retry-prompt only on persistent loop | LangChain pattern |
| Production prevalence | 68% of production LLM agents use ReAct (early 2026 surveys) | [LangChain Tutorials 2026](https://langchain-tutorials.github.io/langchain-react-agent-pattern-2026/) |
| Small model viability | **Honest limitation**: ReAct doesn't magically rescue 8B-parameter agents; complex prompts still break, model may ignore tool output | [Medium — Ollama tool support](https://medium.com/@laurentkubaski/ollama-tool-support-aka-function-calling-23a1c0189bee) |

### §2.2 Why this is the right shape (architectural alignment)

Vision shim invariants that ReAct shim mirrors:

| Vision shim invariant | ReAct shim equivalent |
|---|---|
| Detection at request entry (model name → `is_vision_model` heuristic) | Detection at submit entry (Ollama `/api/show` → `supports_native_tool_calls` cached lookup) |
| Delegation to a capability-providing component | Delegation to in-process ReAct prompt + parser instead of separate model call |
| Text description prepended to user prompt | Tool descriptions injected as system prompt |
| Original (non-vision) model receives the augmented prompt | Original (non-tool) model receives the augmented prompt |
| Result: model "supports" images via gateway | Result: model "supports" tool_calls via gateway |

---

## §3 Tier A — Capability detection

### §3.1 Why use Ollama's `/api/show`, not a heuristic

Web research says: Ollama itself determines tool-calling support by inspecting the model's chat template for the `.Tools` variable. That's the authoritative signal — it tells us what Ollama will do at runtime when we pass `tools` in the request.

A hand-rolled name-matching heuristic (e.g. `qwen3:8b` → no) is brittle: future model name conventions, fine-tunes (`qwen3-something-coder:8b`), and base-vs-instruct variants would all confuse it.

### §3.2 New module

`crates/veronex/src/infrastructure/outbound/ollama/capability.rs` (NEW):

```rust
pub struct OllamaCapability {
    pub supports_native_tool_calls: bool,
    pub supports_vision: bool,
    pub configured_ctx: u32,
}

pub async fn fetch_capability(
    http: &reqwest::Client,
    provider_url: &str,
    model: &str,
) -> Option<OllamaCapability> {
    let endpoint = format!("{}/api/show", provider_url.trim_end_matches('/'));
    let body = serde_json::json!({"name": model});
    let resp = http.post(&endpoint).json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let json: serde_json::Value = resp.json().await.ok()?;
    let template = json["template"].as_str().unwrap_or("");
    let supports_native_tool_calls = template.contains(".Tools") || template.contains("tools");
    // existing heuristic stays as a defense-in-depth fallback for old Ollama versions
    Some(OllamaCapability {
        supports_native_tool_calls,
        supports_vision: template.contains("images"),
        configured_ctx: json["model_info"]["llama.context_length"].as_u64().unwrap_or(0) as u32,
    })
}
```

### §3.3 Cache layer

Provider+model capability is stable per model file. Cache:

| Tier | Key | TTL |
|------|-----|-----|
| L1 | `DashMap<(provider_id, model_name), OllamaCapability>` in `AppState` | process lifetime |
| L2 | Valkey `veronex:capability:{provider_id}:{model}` | 1 day |
| Source of truth | `/api/show` per-provider | re-fetched on cache miss |

### §3.4 Fallback heuristic (defense-in-depth)

When `/api/show` is unreachable (provider down, network error), fall back to the name-pattern heuristic from S18 v1 of this SDD (qwen3-coder/llama3.1+/etc → native; everything else → ReAct shim). Conservative: prefer shim on uncertainty.

### §3.5 Acceptance

- [ ] `cargo build -p veronex` succeeds
- [ ] Unit test: `fetch_capability("qwen3-coder-next-200k:latest")` against mock Ollama returning template with `{{ if .Tools }}` → `supports_native_tool_calls = true`
- [ ] Unit test: template without `.Tools` → `supports_native_tool_calls = false`
- [ ] Integration test on dev: known native models → cache populated with `true`, `qwen3:8b` (which Ollama serves but template support varies) → result reflects actual template

---

## §4 Tier B — ReAct prompt template + renderer

### §4.1 System prompt (locked text)

`crates/veronex/src/infrastructure/outbound/mcp/react_prompt.rs` (NEW):

```rust
pub const REACT_SYSTEM_PROMPT: &str = "\
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
{tool_descriptions}
";

pub fn render_tool_descriptions(tools: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for t in tools {
        let name = t["function"]["name"].as_str().unwrap_or("");
        let desc = t["function"]["description"].as_str().unwrap_or("");
        let params = t["function"]["parameters"].to_string();
        out.push_str(&format!("- {name}: {desc}\n  schema: {params}\n"));
    }
    out
}
```

### §4.2 Acceptance

- [ ] `render_tool_descriptions` produces deterministic markdown for a fixed input (golden test with `insta`)
- [ ] Empty tool list → empty string (caller skips ReAct path)

---

## §5 Tier C — Stream-aware Action parser

### §5.1 State machine

`crates/veronex/src/infrastructure/outbound/mcp/react_parser.rs` (NEW):

```rust
pub enum ReActEvent {
    /// Plain text delta safe to forward to client (final-answer pass-through).
    Text(String),
    /// Tool invocation extracted: ready to execute.
    Action { name: String, args: serde_json::Value },
    /// Final Answer — model declared completion.
    Final(String),
    /// Buffer holding incomplete data, no event yet.
    Pending,
    /// Unparseable structure — caller fail-opens.
    ParseError(String),
}

pub struct ReActParser {
    buf: String,
    in_final: bool,
}

impl ReActParser {
    pub fn new() -> Self { Self { buf: String::new(), in_final: false } }

    pub fn feed(&mut self, chunk: &str) -> Vec<ReActEvent> {
        // 1. If in_final: emit Text(chunk), return.
        // 2. else: append to buf
        // 3. detect "Final Answer:" — switch in_final = true, emit any text after it as Text
        // 4. else: detect "Action:" + "Action Input:" — bracket-count JSON; if balanced, emit Action; trim buf
        // 5. else: emit Pending (still buffering)
    }

    pub fn finish(&mut self) -> Vec<ReActEvent> {
        // EOS: flush remaining buffer.
        // - If buf contains "Final Answer:" → emit Final(rest)
        // - Else if buf contains complete Action → emit Action
        // - Else if buf is empty → []
        // - Else → ParseError("unparseable trailing buffer: ...")
    }
}
```

### §5.2 Bracket-counting JSON extractor

Inside `feed()` — after locating `Action Input:`, scan forward:
- Track depth `d` of `{}` and `[]` brackets, accounting for strings (skip content inside `"..."` while watching for unescaped `"`).
- When `d` returns to 0 and we have `>= {`, the JSON object is complete.
- Parse via `serde_json::from_str`. On parse fail → `ParseError`.

### §5.3 Streaming behavior

- Each chunk arrives via `feed(chunk)` and may yield 0+ events.
- Caller drains events as they arrive; `Text` events are forwarded to SSE client; `Action` events trigger tool execution.
- This composes cleanly with the existing SSE streaming-first architecture (S15) — no full-buffer wait.

### §5.4 Acceptance (parser tests)

| # | Input chunks | Expected events |
|---|-------------|-----------------|
| 1 | `["Thought: I'll search.\nAction: web_search\nAction Input: {\"q\": \"micron\"}\n"]` | `[Action { name: "web_search", args: {"q":"micron"}}]` |
| 2 | `["Thought:...\nAction: ", "web_search\nAction Input:", " {\"q\":\"micron\"}\n"]` | same single Action (split chunks) |
| 3 | `["Action Input: {\"q\":\"a {b}\"}\n"]` | Action with embedded `{}` in string handled correctly |
| 4 | `["Thought: I now know the answer.\nFinal Answer: 답변..."]` | `[Final("답변...")]` |
| 5 | `["Just plain text without any keywords"]` | `[Pending]` then `finish()` → `[ParseError]` |
| 6 | `["Action: web_search\nAction Input: {invalid json"]` (truncated) | `[Pending]`, then `finish()` → `[ParseError]` (caller fail-opens) |
| 7 | `["Final Answer: text", "more text"]` (text after final) | `[Final("text"), Text("more text")]` |

All 7 cases are unit tests in `react_parser::tests`.

---

## §6 Tier D — `bridge::run_loop` path branch + Observation feedback

### §6.1 Routing

In `bridge.rs::run_loop`, before the round loop:

```rust
let use_native = capability_cache.get(provider_id, &model)
    .await
    .map(|c| c.supports_native_tool_calls)
    .unwrap_or_else(|| heuristic_fallback(&model)); // §3.4

if use_native {
    // existing code path (unchanged)
} else {
    return run_loop_react(state, caller, model, messages, mcp_openai_tools, ...).await;
}
```

### §6.2 ReAct loop

`bridge.rs::run_loop_react` (NEW function, ~150 LOC):

```text
1. Inject system prompt: REACT_SYSTEM_PROMPT with rendered tool_descriptions.
2. for round in 0..MAX_ROUNDS:
     a. submit job (NO tools field — text-pattern model)
     b. open SSE / stream consumer
     c. parser = ReActParser::new()
     d. for chunk in stream: events = parser.feed(chunk)
        - on Text: forward to client SSE
        - on Action: execute via existing bridge::execute_calls
                     append "Observation: <result>" to messages
                     break inner stream consumer (no need to wait for more)
        - on Final: forward, mark loop done
        - on ParseError: log, fail-open — treat all accumulated text as Final Answer
     e. if Final or ParseError: break outer round loop
3. write per-round S3 turns (S16 invariant — runner already handles this for stream_tokens path)
```

### §6.3 Reuse existing infrastructure

- `execute_calls` / `execute_one` (`bridge.rs:512-600`) — unchanged; ReAct path feeds the same Action object format.
- `loop_detection` (signature counting) — applied to ReAct Actions identically.
- `MAX_ROUNDS` (cap_points / 5) — same enforcement.
- `intermediate_job_ids` cleanup — same.
- S3 persist (S16) — runner already writes per-round; no bridge-side write needed.

### §6.4 Acceptance

- [ ] `cargo build -p veronex` succeeds
- [ ] Unit test: native path with mock capability → original behavior (regression check)
- [ ] Unit test: ReAct path with mock capability + mock LLM stream → calls `execute_calls` once per parsed Action
- [ ] Unit test: parser ParseError → fail-open, no panic

---

## §7 Tier E — Test matrix

| # | Test | Target |
|---|------|--------|
| 1 | Capability `/api/show` parses native template | Tier A |
| 2 | Capability fallback heuristic when `/api/show` fails | Tier A |
| 3 | Capability cache hits L1 then L2 | Tier A |
| 4 | `render_tool_descriptions` golden | Tier B |
| 5 | Parser cases 1-7 from §5.4 | Tier C |
| 6 | `run_loop` native path = unchanged behavior | Tier D |
| 7 | `run_loop_react` mock 2-round flow | Tier D |
| 8 | Loop detection in ReAct path | Tier D |

---

## §8 Live verification (dev)

### §8.1 Setup

- `qwen3:8b` already on dev.
- MCP server `veronex-mcp-dev.verobee.com` enabled.
- Test prompt: "오늘 마이크론 주가에 대해 알려줘" (the failed `conv_337tGMRdShMz35be763bn` prompt).

### §8.2 PASS conditions

| # | Check |
|---|-------|
| L1 | Capability cache shows `qwen3:8b` → `supports_native_tool_calls = false` (assuming Ollama template lacks `.Tools`) |
| L2 | Bridge log: `mcp.path = react` for this request |
| L3 | Stream contains `Action: web_search` |
| L4 | `web_search` actually executed (DB row in `mcp_loop_tool_calls`) |
| L5 | Stream contains `Observation: ...` (injected by bridge) |
| L6 | Stream ends with `Final Answer: <Korean text about Micron>` |
| L7 | Final answer length > 50 chars + contains "마이크론" |
| L8 | Dashboard detail GET on the round shows `result_text` populated |

### §8.3 Negative — native model unaffected

`qwen3-coder-next-200k:latest` same prompt:
- Capability cache → `true` → native path
- Logs / behavior identical to current S15/S16

---

## §9 CDD-sync (planned)

### §9.1 `docs/llm/inference/mcp.md`

Add section after "Architecture" diagram:

```
## Shim path: ReAct fallback for non-native models

Native path is default for models whose Ollama template includes the
`.Tools` block (Qwen3-Coder, Llama 3.1+, Mistral-instruct-v0.3+, etc.).
Models without native tool template fall through to the ReAct shim:

[diagram showing ReAct loop: prompt template → stream → parser → execute_calls → observation feed-back]

Capability detection: `OllamaCapability::supports_native_tool_calls`
sourced from `/api/show` template inspection, cached in DashMap (process)
and Valkey (1-day TTL). Heuristic fallback applies when /api/show is
unreachable.
```

### §9.2 `docs/llm/inference/lab-features.md`

Add to "Gateway shims" table:

| Capability | Shim mechanism | Trigger |
|---|---|---|
| Vision | `analyze_images_for_context` delegates to vision_model | non-vision model + image input |
| **Tool calling (NEW)** | **`react_parser` + ReAct system prompt; executes via existing `execute_calls`** | **non-native-tool-calling model + MCP active** |

### §9.3 Acceptance

- [ ] `grep -n "ReAct" docs/llm/inference/mcp.md` returns the new section
- [ ] `grep -n "tool calling" docs/llm/inference/lab-features.md` returns the new row

---

## §10 Honest limitations (research-acknowledged)

ReAct shim **does not magically rescue 8B-parameter agents**. Per [Medium — Ollama tool support](https://medium.com/@laurentkubaski/ollama-tool-support-aka-function-calling-23a1c0189bee): "with more complex prompts, function calling with small models (8B parameters or so) starts breaking, and sometimes the model decides to do the calculation on its own and completely ignores the tools output." Same caveat applies to ReAct.

The shim's value: enables tool-calling for **the long tail of mid-size models that emit text-pattern actions reliably but lack native `tool_calls` fine-tuning** — Mistral-7B-Instruct-v0.2, older Llama 3 base, community fine-tunes. For 8B-class models, agentic reliability is fundamentally bounded by model size. Operator-level guidance (deselect 8B from MCP-enabled rotations) remains the recommended practice for unreliable models.

---

## §11 Resume rule recap

If `infrastructure/outbound/ollama/capability.rs` doesn't exist: Tier A. If `react_prompt.rs` / `react_parser.rs` don't exist: Tier B/C. If `run_loop_react` doesn't exist in `bridge.rs`: Tier D. If §8 PASS conditions unverified: live verify pending.

## Sources

- [Ollama Tool Calling — official docs](https://docs.ollama.com/capabilities/tool-calling)
- [DeepWiki — Ollama Tool Calling and Function Execution](https://deepwiki.com/ollama/ollama/7.2-tool-calling-and-function-execution)
- [LangChain — ReActJsonSingleInputOutputParser source](https://api.python.langchain.com/en/latest/_modules/langchain/agents/output_parsers/react_json_single_input.html)
- [LangChain Tutorials — ReAct Agent Pattern 2026](https://langchain-tutorials.github.io/langchain-react-agent-pattern-2026/)
- [Medium — Ollama Tool Support / Function Calling](https://medium.com/@laurentkubaski/ollama-tool-support-aka-function-calling-23a1c0189bee)
- [Sitepoint — Hybrid Cloud-Local LLM Architecture 2026](https://www.sitepoint.com/hybrid-cloudlocal-llm-the-complete-architecture-guide-2026/)
- [Mercity — ReAct Prompting & Agentic Systems](https://www.mercity.ai/blog-post/react-prompting-and-react-based-agentic-systems/)
- [LeewayHertz — ReAct vs Function Calling](https://www.leewayhertz.com/react-agents-vs-function-calling-agents/)
- [Qwen Function Calling docs](https://qwen.readthedocs.io/en/latest/framework/function_call.html)
- [arxiv 2405.13966 — Brittle Foundations of ReAct](https://arxiv.org/html/2405.13966v1)
