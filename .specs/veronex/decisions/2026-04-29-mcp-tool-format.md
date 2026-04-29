# ADR — MCP Tool-Call Format: Native Function Calling, Not ReAct

> Status: accepted | Created: 2026-04-29 | Decision driver: user question after `conv_337tGMRdShMz35be763bn` showed `qwen3:8b` ignoring attached MCP tools

## Question

Why does veronex's MCP integration (S11/S12/S15/S16) use **native OpenAI-style `tool_calls`** and not the **ReAct prompt pattern** (`Thought:` / `Action:` / `Observation:` text)? Should we add ReAct as a fallback for models that don't reliably emit native tool_calls?

## Triggering observation

`conv_337tGMRdShMz35be763bn` (job `019dd816`, model `qwen3:8b`, dev 2026-04-29 07:14 UTC):

| Property | Value |
|----------|-------|
| MCP intercept fired | ✅ `mcp_loop_id = 86f244d3-...` assigned |
| MCP server / tools indexed | ✅ 4 tools, Vespa sync OK |
| `bridge.run_loop` ran | ✅ |
| Native `tool_calls` emitted by model | ❌ **0** |
| Result | English text "I don't have access to real-time financial data..." |

Same prompt on `qwen3-coder-next-200k:latest` 06:44 UTC: `MCP round complete round=0 mcp_calls=1` — `web_search` was called natively. Confirms the failure is per-model, not per-format.

## Investigation

### Codebase verification (current native path)

| Component | File | Behavior |
|-----------|------|----------|
| Tool list assembly | `mcp/bridge.rs:170-207` | Vespa Top-K → OpenAI-format `[{"type":"function","function":{...}}]` |
| Submission to provider | `ollama/adapter.rs:463-555` | Forwards `tools` to Ollama `/api/chat` (native field) |
| Tool-call read-back | `ollama/adapter.rs:615-629` | Reads `message.tool_calls` from Ollama JSON, emits as `StreamToken { tool_calls: ... }` |
| SSE emit | `openai_handlers.rs::mcp_ollama_chat` | Forwards as OpenAI-compat `delta.tool_calls` chunks |

→ End-to-end **native function-calling pipeline**. No text-pattern parsing anywhere.

### Web research (2026)

| Source | Finding | Implication |
|--------|---------|-------------|
| [Qwen Function Calling docs](https://qwen.readthedocs.io/en/latest/framework/function_call.html) | "For reasoning models like Qwen3, **it is not recommended to use tool call template based on stopwords, such as ReAct**, because the model may output stopwords in the thought section" | **ReAct is officially deprecated for our primary model family** |
| [Qwen Function Calling docs](https://qwen.readthedocs.io/en/latest/framework/function_call.html) | "Hermes-style tool use is recommended for Qwen3" — Hermes is still **native** structured output, not ReAct | Native is the recommendation |
| [arxiv 2405.13966 — Brittle Foundations of ReAct](https://arxiv.org/html/2405.13966v1) | "ReAct becomes brittle when considering variability in prompt designs … underscoring its limitations in handling diverse input prompts" | ReAct lacks robustness for production |
| [DEV — ReAct vs Tool Calling](https://dev.to/parth_sarthisharma_105e7/react-vs-tool-calling-why-your-llm-should-decide-but-never-execute-cp3) | "Modern models like Claude, GPT-4+, and Gemini have tool calling built into the API layer" — "ReAct produces plain text reasoning, making it powerful but **not safe or structured enough for production**" | Industry has moved past ReAct |
| [LeewayHertz — ReAct vs Function Calling](https://www.leewayhertz.com/react-agents-vs-function-calling-agents/) | "Tool-calling agents are often a more efficient choice with structured tool calls and fewer syntax errors" | Production preference is native |

### Why qwen3:8b didn't call tools (despite native support)

Per the Qwen3 technical docs cited above:
- The 8B variant supports native tool calling.
- Qwen3 evaluates a **ToolUse benchmark** in single/multi-turn/multi-step tool calling.
- "It is **not guaranteed** that the model generation will always follow the protocol even with proper prompting or templates" — i.e., model SIZE / TRAINING is the limiting factor, not the format.

In `conv_337tGMRdShMz35be763bn`, `qwen3:8b` simply did not invoke the available tool — a model-capability outcome, not a missing-format outcome. The 200K-context Qwen3 variant (much larger) on the same prompt at 06:44 UTC successfully called `web_search`.

## Decision

**Keep native OpenAI-style `tool_calls`. Do NOT add ReAct support, even as a fallback.**

Rationale (cumulative):

1. **Industry convergence**: native function calling is the production standard across OpenAI / Anthropic / Gemini / Mistral / Qwen as of 2026.
2. **Model-author guidance**: Qwen team (our primary model family) **explicitly warns against ReAct templates** for Qwen3 — adding ReAct would directly contradict upstream guidance and degrade quality.
3. **Research evidence**: ReAct is brittle under prompt-design variance (arxiv 2024).
4. **Implementation cost**: ReAct fallback would mean parallel parser path in `bridge.rs`, mid-stream buffering for "Action:" detection, escape-character handling, and a maintenance burden — for negative quality benefit on Qwen3.
5. **Streaming compatibility**: native `tool_calls` chunks delta into SSE cleanly (already implemented in `adapter.rs:615-629`); ReAct text-pattern requires buffer-then-parse, breaking the streaming-first architecture (S15).

The qwen3:8b failure is **not solved by changing format** — solved by:
- **Operator action**: don't expose 8B-class models to MCP-enabled keys (dashboard toggle, no code change).
- **Better model selection**: Qwen3-Coder-Next-200K and similar large variants reliably call tools.

## Consequences

### Accepted
- Smaller models (≤8B) may fail to invoke MCP tools even when they should. Mitigated by model-selection guidance, not format change.
- Maintenance: one well-tested code path (native), no parallel ReAct path.

### Reversal triggers (when to revisit)
This decision should be re-opened ONLY if:
- A future Qwen / target-model release ships ReAct as the **recommended** template (currently deprecated).
- We observe ≥3 distinct production scenarios where a strategically required model lacks reliable native tool-calling.
- Industry standard shifts back toward text-pattern (no signal of this).

### Alternatives (not chosen — listed for completeness)

| Alternative | Why rejected |
|-------------|--------------|
| Add ReAct fallback in bridge | Officially deprecated for Qwen3; brittle; doubles maintenance burden |
| Hermes-style template (Qwen3-specific) | Still native, but model-specific; Ollama already translates OpenAI tools to model-specific format internally — no benefit at our layer |
| Stronger system prompt nudging tool use | Marginal effect on 8B; Qwen3 is already tool-trained natively |
| Remove qwen3:8b from MCP-enabled selection | **Recommended operator action**, no code change |

## Action items

| # | Item | Owner | Type |
|---|------|-------|------|
| 1 | Operator: in dev dashboard, deselect `qwen3:8b` from MCP-enabled provider model list (or document in lab settings that smaller models may skip MCP tools) | Operator | Configuration |
| 2 | Optional: surface "expected MCP-enabled" warning in test panel UI when small models are selected | Frontend | Future, not blocking |

No SDD / code change required for this ADR.

## Sources

- [Qwen Function Calling — official docs](https://qwen.readthedocs.io/en/latest/framework/function_call.html)
- [Qwen3 Technical Report (arxiv 2505.09388)](https://arxiv.org/pdf/2505.09388)
- [arxiv 2405.13966 — Brittle Foundations of ReAct](https://arxiv.org/html/2405.13966v1)
- [DEV Community — ReAct vs Tool Calling](https://dev.to/parth_sarthisharma_105e7/react-vs-tool-calling-why-your-llm-should-decide-but-never-execute-cp3)
- [LeewayHertz — ReAct agents vs Function Calling agents](https://www.leewayhertz.com/react-agents-vs-function-calling-agents/)
- [Mercity — ReAct Prompting and ReAct based Agentic Systems](https://www.mercity.ai/blog-post/react-prompting-and-react-based-agentic-systems/)
