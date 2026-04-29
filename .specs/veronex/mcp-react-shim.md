# SDD: MCP ReAct Shim (gateway tool-calling fallback for non-native models)

> Status: planned | Change type: **Add** (new gateway shim mirroring vision-shim pattern) | Created: 2026-04-29 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/inference/lab-features.md` · `crates/veronex/src/infrastructure/inbound/http/inference_helpers.rs::analyze_images_for_context` (vision shim — pattern reference)
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S18 (to add)
> **Resume rule**: every section is self-contained. Future sessions reading this SDD alone must continue from the last unchecked box.

---

## §0 Quick-resume State

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| A — Capability detection (`supports_native_tool_calls`) | [ ] | `feat/mcp-react-fallback` | — | — |
| B — ReAct prompt template + output parser | [ ] | (same) | — | — |
| C — bridge.rs integration (route to native vs ReAct path) | [ ] | (same) | — | — |
| D — Tests (unit + parser fuzz) | [ ] | (same) | — | — |
| CDD-sync (`mcp.md` adds shim section) | [ ] | — | — | — |
| Live verify (dev) — non-tool-calling model executes MCP tool via ReAct | [ ] | — | — | — |

---

## §1 Problem

### Project pattern: gateway shims for missing model capabilities

veronex (Vero+Nexus) is a **gateway** that lets ANY underlying Ollama model participate in capabilities the model itself may not natively support. The existing **vision shim** (`inference_helpers.rs:150 analyze_images_for_context`) is the canonical example:

```
non-vision model + image input
   ↓
gateway delegates image → vision_model (qwen3-vl:8b default) → text description
   ↓
prepend description to user prompt → send to non-vision model
   ↓
non-vision model can answer about the image
```

The gateway's **architectural promise** is: feature-richness should depend on the gateway's shims, not on each underlying model's intrinsic capabilities.

### Missing shim: native tool-calling

The MCP integration (S11/S12/S15/S16) implemented end-to-end native OpenAI-style `tool_calls`:

| Code site | Behavior |
|-----------|----------|
| `mcp/bridge.rs:170-207` | Builds OpenAI-format tools list, passes to provider |
| `ollama/adapter.rs:463-629` | Forwards `tools` to Ollama `/api/chat`, reads back `message.tool_calls` |
| `openai_handlers.rs::mcp_ollama_chat` | Streams `delta.tool_calls` back to client |

→ Models that don't natively emit `tool_calls` get **no MCP capability through this gateway**, even if they're capable of agentic behavior via ReAct prompt patterns.

### Concrete failure observed

`conv_337tGMRdShMz35be763bn` (job `019dd816`, model `qwen3:8b`, dev 2026-04-29 07:14 UTC):
- MCP intercept fired (`mcp_loop_id` assigned).
- 4 MCP tools attached to `tools[]`.
- Model returned text "I don't have access to real-time financial data..." — **0 tool_calls emitted**.
- Same prompt on `qwen3-coder-next-200k:latest` (06:44 UTC) emitted `tool_calls` natively.

Smaller / older / less-tool-trained models simply do not reliably emit native `tool_calls`. The gateway's promise is broken for them today.

### Decision (the actual one)

**Add a ReAct shim to the gateway**, mirroring the vision-shim pattern. When the underlying model does not reliably emit native `tool_calls`, the gateway:

1. Injects a system prompt teaching ReAct format (`Thought:` / `Action:` / `Action Input:` / `Observation:`).
2. Streams the model's text response.
3. Parses for `Action: tool_name` + `Action Input: {...}` patterns.
4. Executes the tool via the existing MCP machinery (`bridge::execute_calls`).
5. Appends `Observation: <result>` to the messages and continues.
6. Loops until the model emits `Final Answer:` or the loop bound (`MAX_ROUNDS`) is hit.

The native path remains the default for models that do support `tool_calls` natively (per Qwen team's recommendation for Qwen3 — see §3.4 below).

---

## §2 Root cause analysis

### Why the gap exists today

The MCP integration (S11/S12 earlier this Q2) targeted a specific model family (Qwen3-Coder, Gemini) that all natively support `tool_calls`. The architecture assumed every MCP-eligible model would emit native `tool_calls`. No fallback was built because the initial scope didn't surface the gap. The gateway's "shim for missing capabilities" pattern (visible in vision support) was not extended to tool-calling.

### Why this is a real problem

veronex's value proposition (per `.ai/README.md`): "Autonomous intelligence scheduler/gateway for N Ollama servers". The gateway aggregates a heterogeneous model fleet; users select models from the dashboard. If only a subset of the fleet can participate in MCP, the gateway's abstraction leaks — users have to know which models support MCP and which don't, defeating the gateway pattern.

---

## §3 Solution: ReAct shim mirroring vision shim

### §3.1 Capability detection (Tier A)

Add `is_native_tool_calling_model(model_name: &str) -> bool` next to `is_vision_model` in `inference_helpers.rs`. Heuristic on model-name substrings (initial pass, can be promoted to a per-model DB column later if needed):

| Pattern | Native tool_calls support |
|---------|---------------------------|
| `qwen3-coder*` | yes (Qwen3 large variants reliably emit) |
| `qwen3:32b` and larger Qwen3 | yes (per Qwen team) |
| `qwen3:8b` and smaller Qwen3 | **no** (per observed failure + Qwen tech report's reliability caveat) |
| `qwen2.5-instruct*`, `qwen2.5-coder*` | yes |
| `llama3.1*`, `llama3.2*`, `llama3.3*` | yes |
| `llama3:*` (3.0 base) | no |
| `mistral-instruct-v0.3*`, `mixtral-instruct*` | yes |
| `hermes-*`, `nous-*`, `command-r*` | yes |
| anything else (older / community / unknown) | **no** (default: shim) |

Default unknown → no, so the shim is opt-in by recognized native models. Conservative: better to use ReAct shim when uncertain than to send tools to a model that ignores them.

### §3.2 ReAct prompt template (Tier B)

Standard ReAct preamble injected as a system message when the shim path is active:

```
You are a helpful assistant with access to tools. When you need to use a tool,
respond in this exact format:

  Thought: <your reasoning about which tool to use>
  Action: <tool_name>
  Action Input: <JSON object with the tool arguments>

After the system runs the tool, you will receive:

  Observation: <tool result>

You can chain multiple Thought/Action/Action Input/Observation cycles.
When you have enough information to answer, respond:

  Thought: I now know the answer.
  Final Answer: <your answer to the user>

Available tools:
{tool_descriptions}
```

`{tool_descriptions}` is the existing OpenAI-format tools list rendered as markdown bullets:

```
- web_search(args: {query: string, count?: number}) — Search the web for information.
- ...
```

### §3.3 Output parser (Tier B)

Stream-aware parser that buffers until a complete `Action:` block is received. Critical correctness rules:

| Concern | Handling |
|---------|----------|
| Multi-line `Action Input` | Buffer until `Observation:` or `Final Answer:` keyword on a new line |
| JSON in `Action Input` may contain newlines | Use bracket-counting JSON parser; do not split on bare newlines |
| Model emits `Final Answer:` mid-stream | Forward subsequent text directly to client (no further parsing) |
| Model emits no `Action:` (just text) | Treat as final answer; surface to client |
| Model loops (same Action repeatedly) | Existing loop-detection in `bridge.rs:368 loop_detected` — reuse, no change |
| Stream cancelled mid-Action | Tier B parser writes whatever was buffered to the per-round S3 turn (Tier B from S16 already covers this) |

### §3.4 Path selection in bridge.rs (Tier C)

`bridge::run_loop` currently always builds `tools_json` for native path. Add at the start:

```rust
let use_native = is_native_tool_calling_model(&model);
if use_native {
    // existing native path (unchanged)
} else {
    // new ReAct path:
    // 1. inject system prompt with ReAct template + tool descriptions
    // 2. submit each round via stream_tokens (NO tools attached)
    // 3. parse text output for Action: blocks
    // 4. execute via execute_calls (same as native)
    // 5. append "Observation: ..." text-pattern back into messages[]
}
```

Both paths converge on `execute_calls()` which is unchanged — MCP tool invocation is independent of how the model expressed the call.

### §3.5 Why this is the right shape (research-backed)

| Source | Finding |
|--------|---------|
| [Qwen Function Calling docs](https://qwen.readthedocs.io/en/latest/framework/function_call.html) | "For reasoning models like Qwen3, it is not recommended to use tool call template based on stopwords, such as ReAct" — applies to **Qwen3 reasoning models specifically**, not to all underlying models. Heuristic correctly routes Qwen3 to native path. |
| [arxiv 2405.13966 — Brittle Foundations of ReAct](https://arxiv.org/html/2405.13966v1) | ReAct is brittle under prompt-design variance. **Mitigation**: keep prompt template stable + defensive parser + fail-open (treat unparseable as Final Answer). |
| [Mercity — ReAct Prompting](https://www.mercity.ai/blog-post/react-prompting-and-react-based-agentic-systems/) | ReAct's reasoning-action-observation cycle is the canonical fallback for models without native function calling. Industry uses ReAct as a compatibility shim, not as a replacement for native. |
| [LeewayHertz — ReAct vs Function Calling](https://www.leewayhertz.com/react-agents-vs-function-calling-agents/) | "ReAct works with virtually any LLM that can follow text-based instructions" — confirms shim viability for the open-model long tail. |

→ The hybrid (native preferred + ReAct shim for non-native) is the industry consensus when supporting a heterogeneous model fleet. This is what veronex's gateway pattern requires.

---

## §4 Files to modify (planned)

| File | Change |
|------|--------|
| `crates/veronex/src/infrastructure/inbound/http/inference_helpers.rs` | Add `is_native_tool_calling_model(model_name)` next to `is_vision_model` |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | Branch in `run_loop`: native path (existing) vs ReAct path (new) |
| `crates/veronex/src/infrastructure/outbound/mcp/react_parser.rs` | NEW — stream-aware Action/Action Input/Observation parser |
| `crates/veronex/src/infrastructure/outbound/mcp/react_prompt.rs` | NEW — system-prompt builder rendering tool descriptions |
| `docs/llm/inference/mcp.md` | Add "Shim path: ReAct fallback" section + heuristic table |
| `docs/llm/inference/lab-features.md` | Document the shim alongside vision shim as gateway-pattern features |

---

## §5 Tests (Tier D)

| Test | Where | Asserts |
|------|-------|---------|
| `is_native_tool_calling_model` heuristic | unit | known patterns return true; unknowns return false (conservative) |
| ReAct parser: single Action | unit | extracts tool name + JSON args |
| ReAct parser: Action Input with embedded newlines / nested JSON | unit | bracket-counting parses correctly |
| ReAct parser: Final Answer | unit | switches to passthrough mode |
| ReAct parser: incomplete Action (cancelled stream) | unit | partial-buffer flushed to S3 via existing Tier B helper |
| ReAct parser: model emits raw text (no Action) | unit | treated as Final Answer |
| Loop detection: same Action twice | integration | reuses existing `bridge::loop_detected` |
| Native model: Qwen3-Coder-200K | integration | uses native path, not shim |
| Non-native model: simulated qwen3:8b stub | integration | uses ReAct path, completes loop |

---

## §6 Live verification (dev)

### §6.1 Setup

- A non-native model installed on dev Ollama (e.g., `qwen3:8b`).
- One MCP server registered (currently `veronex-mcp-dev.verobee.com` with web_search).
- Test prompt: "오늘 마이크론 주가에 대해 알려줘" (the failed `conv_337tGMRdShMz35be763bn` prompt).

### §6.2 PASS conditions

| # | Check |
|---|-------|
| L1 | `is_native_tool_calling_model("qwen3:8b")` returns false → ReAct path activates |
| L2 | bridge log contains `MCP shim: ReAct path active` (new log line) |
| L3 | Model output contains `Action: web_search` parsed correctly |
| L4 | `web_search` actually invoked (existing `execute_one` path), result in DB |
| L5 | Final assistant text non-empty, contains relevant content (Korean answer about Micron stock) |
| L6 | dashboard detail GET on the final round returns `result_text` length > 50, `tool_calls_json` populated with the parsed Action call |

### §6.3 NEG: native model still uses native path

Same prompt on `qwen3-coder-next-200k:latest`:
- `is_native_tool_calling_model(...)` returns true
- Bridge uses native path (no ReAct prompt injected)
- Behavior unchanged from current S15/S16

---

## §7 CDD-sync (planned)

### `docs/llm/inference/mcp.md`

Add a new section after the existing "Architecture" diagram:

```
## Shim path: ReAct fallback for non-native models

veronex acts as an MCP gateway for any underlying Ollama model. Models that
emit native `tool_calls` use the native path (above). Models that don't
(per `is_native_tool_calling_model`) use the ReAct shim:

[diagram + reference to react_parser.rs / react_prompt.rs]
```

### `docs/llm/inference/lab-features.md`

Add a row to the "Gateway shims" pattern documentation:

| Capability | Shim mechanism | Trigger |
|-----------|----------------|---------|
| Vision  | `analyze_images_for_context` delegates to vision_model | non-vision model + image input |
| **Tool calling (NEW)** | **ReAct prompt template + parser, executes via existing `execute_calls`** | **non-native-tool-calling model + MCP active** |

---

## §8 Acceptance summary

The shim is correct when:
- Native path behavior is byte-identical for native models (regression: zero).
- Non-native model can complete an MCP loop (e.g., `qwen3:8b` calls `web_search` and returns Korean answer).
- Stream parser handles cancel mid-Action without losing accumulated state (uses existing Tier B mechanics).
- Heuristic-based detection has explicit override path (lab_settings.force_react_shim or similar) for operator pinning.

---

## §10 Follow-ups (not in this SDD)

- Promote heuristic detection to a per-model DB column (`provider_selected_models.supports_native_tool_calls bool`) once we have ≥10 models in production. Heuristic suffices for opening rounds.
- Operator-side pinning (force-shim or force-native per model) only if observation reveals heuristic mistakes.
- Per-model ReAct prompt customization (e.g., Korean-language preamble for Korean-speaking models) — only if quality observation justifies it.

---

## §11 Resume rule recap

If §0 boxes unchecked but `is_native_tool_calling_model` exists in `inference_helpers.rs`: Tier A is done. If `react_parser.rs` exists and tests pass: Tier B done. If `bridge.rs::run_loop` has a branch on `is_native_tool_calling_model`: Tier C done. If §6.2 PASS conditions hold on dev: live verify done. Each tier has its own acceptance — never trust an unchecked box without re-running its acceptance.

This SDD specifies WHAT to build and WHY (with research backing) — the implementation details (parser state machine, prompt wording iteration) are intentionally left for the implementation phase to refine in code review.
