# SDD: MCP unification under constrained decoding (single forced-JSON path)

> Status: planned | Change type: **Architecture** (path collapse) | Created: 2026-05-04 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/flows/mcp.md`
> Predecessors (collapsed by this SDD): `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` (S23) · `.specs/veronex/mcp-synthesis-round.md` (S24)

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — Drop `heuristic_supports_native` gate at `bridge.rs:284-305`; route every MCP-routed request through the constrained-decoding path | [ ] | — | — |
| B — Rename `run_loop_forced_json` → `run_loop`; delete the legacy native MCP `run_loop` body (lines 165-774) | [ ] | — | — |
| C — Delete S23 convergence-boundary block (lines 335-388, 405) and its supporting code paths inside the legacy native path | [ ] | — | — |
| D — Delete S24 synthesis fallback (lines 591-661) + `extract_tool_results` + `build_synthesis_messages` helpers | [ ] | — | — |
| E — Strengthen `forced_json` schema: add `{action:"refuse", reason}` oneOf branch so the model has a non-tool exit when no tool fits; preserve `allow_final` round-0 gating | [ ] | — | — |
| F — Update bridge tests; add regression fixture for `conv_33AfPaddqdXSiqIHX081T` (Turn 1/3 disclaimer-after-tools, Turn 2/4 zero-rounds escape) | [ ] | — | — |
| G — Update CDD: rewrite `inference/mcp.md` Architecture + Protections, rewrite `flows/mcp.md` `run_loop()` diagram; remove S23/S24 sections | [ ] | — | — |
| H — Live verify on dev: re-run conv_33Af reproducer; expect zero disclaimer responses, zero zero-round responses, JSON validity 100% | [ ] | — | — |

---

## §1 Problem (verified 2026-05-04, dev cluster, conv_33AfPaddqdXSiqIHX081T)

The MCP bridge has **two ReAct loops** behind a model-family heuristic:

```
chat_completions → mcp_ollama_chat → bridge.run_loop()
                                       ├── round 0: budget gate + tool list build
                                       └── if !heuristic_supports_native(model) → run_loop_forced_json (constrained decoding)
                                          else                                   → continues in run_loop (native trust path)
```

`heuristic_supports_native` returns `true` for Qwen3-Coder, Qwen2.5-Instruct/Coder, Llama 3.1+, Mistral-instruct-v0.3+, Hermes/Nous, Command-R, Gemma 4. Those families take the **native path** which trusts the model to decide when to call tools and when to answer. Forced-JSON path applies constrained decoding (`response_format: json_schema` → Ollama `format` → llama.cpp GBNF logit mask).

Live conversation `conv_33AfPaddqdXSiqIHX081T` (Qwen3-Coder, native path) showed all four turns failing the gateway contract:

| Turn | Question | Rounds executed | Result |
|------|----------|-----------------|--------|
| 1 | 미장 섹터 정리 | 5 (web_search ×3 + weather ×2) | "저는 실시간 데이터에 직접적 접근 권한 없으며..." disclaimer despite 14KB of search results in context |
| 2 | 추천 종목/목표가 | **0** | "실시간 종목 추천 제한됩니다" — no tool tried |
| 3 | 웹검색으로 종목 찾아 | 5 (NVDA/FSLR/MRNA/PLTR/AMD target prices) | "웹검색 기능이 비활성화된 상태" while having 5 successful searches in messages[] |
| 4 | 웹검색 가능한데 헛소리? | **0** | "지금 바로 검색하겠습니다" — promised, did not call |

Each turn is its own ReAct cycle (CDD `inference/mcp.md` §single-writer policy). Turns 1/3 ran the native loop to `MAX_ROUNDS=5` and still produced disclaimer text. Turns 2/4 produced disclaimer text on round 0 and broke immediately at `bridge.rs:515` (`if mcp_calls.is_empty() { break; }`) without trying any tool.

S23 convergence-boundary (PR #128) and S24 synthesis fallback (PR #129) only fire on `content.is_empty()`. Disclaimer text is non-empty, so neither protection engaged.

The native path's failure mode is structural: it trusts the model's training-cutoff disclaimer reflex over the tool results sitting in the messages array. Patching with more sentinel-detection logic (S23, S24) accumulates band-aids without addressing the root cause — that the gateway delegates the dispatch contract to the model rather than enforcing it.

---

## §2 Root cause

CDD `inference/mcp.md` §"Why this honors the gateway promise" defines veronex's stated principle:

> the gateway **enforces** the dispatch contract: the model literally cannot emit non-JSON or invalid arguments. Tool dispatch is deterministic across every Ollama-served model regardless of fine-tuning.

This holds for the forced-JSON path (allow_final=false at round 0 schema → no logit space for "I don't have access to real-time data" before a tool is tried; once tool result is in context, allow_final=true).

The native path **violates** this principle by design: it sends `tools: [...]` and lets the model self-decide whether to emit `tool_calls` or `content`. Native tool-calling fine-tunes are correlated with tool-use competence, but not sufficient — Qwen3-Coder's RAG-disclaimer reflex demonstrably overrides its tool-use behaviour even after multiple tool successes (conv_33Af Turn 1/3).

The patches accumulated to bandage this:

| Patch | What it solves | What it doesn't |
|-------|----------------|-----------------|
| S23 convergence boundary (PR #128) | model emits `<tool_call>` tokens on the final round despite no tools schema (Qwen3-Coder #475 history-mimicry) | model emits disclaimer text on any earlier round; `content.is_empty()` gate |
| S24 synthesis round (PR #129) | loop exits with empty content + tool results present | non-empty disclaimer content; round-0 zero-tool exits |
| `MAX_ROUNDS=5` | infinite loops | none of the conv_33Af failure modes |
| Loop detect ×3 | identical-call tight loops | none of the conv_33Af failure modes |

The core observation: every native-path patch is reactive (post-failure detection). Constrained decoding is preventive (logit-mask before generation). The two-path split makes veronex carry the cost of the reactive approach for ~80% of model traffic (Qwen/Llama families dominate the allow-list) while the preventive approach already exists in the same module.

---

## §3 Solution

### §3.1 Tier A — Drop the heuristic gate

`bridge.rs:284-305` collapses to:

```rust
// MCP path is universally constrained-decoding. The native tool_calls
// branch is retained ONLY for non-MCP customer tools (handled at the
// chat_completions handler layer when should_intercept() == false).
return self.run_loop_forced_json(
    state, caller, model, messages, all_tools,
    conversation_id, stop, seed, response_format,
    frequency_penalty, presence_penalty,
    allowed_servers, max_rounds,
).await;
```

The `heuristic_supports_native` function stays in `ollama::capability` for non-MCP code paths (callers outside `run_loop` may still need the signal — verify in Tier A grep).

### §3.2 Tier B — Rename and delete

| Action | File | Lines |
|--------|------|-------|
| Rename `run_loop_forced_json` → `run_loop` (the public entry point) | `bridge.rs` | 790 |
| Delete legacy native `run_loop` body | `bridge.rs` | 165-774 (≈610 lines, minus the prelude that now belongs to the new public `run_loop`) |
| Move budget gate + tool list build prelude (lines 201-275) to the head of the unified `run_loop` | `bridge.rs` | — |
| Update `mcp_ollama_chat` callsite signature (no change — already calls `bridge.run_loop`) | `openai_handlers.rs:842` | none |

### §3.3 Tier C — Delete S23 convergence boundary

```
bridge.rs:335-388  delete entire convergence_boundary block
bridge.rs:405      replace `tools: if convergence_boundary { None } else { tools_json.clone().map(Value::Array) }` callsite — handled by the unified path's `tools: None` (always)
```

Constrained decoding makes S23 redundant: the schema's `allow_final` mechanism is the proper convergence control (allow_final=true after ≥1 tool result → model can choose to terminate cleanly via `{"action":"final","answer":"..."}`).

### §3.4 Tier D — Delete S24 synthesis round

```
bridge.rs:591-661  delete S24 synthesis fallback block in legacy run_loop
bridge.rs:1568-1624 (approx) delete extract_tool_results + build_synthesis_messages helpers
```

Constrained decoding makes S24 redundant: `final` branch always allowed once tool results exist; model cannot hide in disclaimer prose because the `final.answer` string itself is generated under the schema. If the model has nothing useful to synthesize, that's a tool-coverage problem, not a degenerate-loop problem.

### §3.5 Tier E — Add `refuse` branch to forced-JSON schema

Currently `forced_json::build_forced_json_schema` emits `oneOf [tool branches..., final?]`. Add a third terminal branch when `allow_final=true`:

```json
{
  "type":"object",
  "properties": {
    "action": {"const":"refuse"},
    "reason": {"type":"string", "minLength": 8}
  },
  "required": ["action", "reason"],
  "additionalProperties": false
}
```

`reason` returned to the caller as the assistant `content` with a known sentinel prefix `"REFUSED: "` so the UI can render it distinctly. This gives the model a structurally-correct exit when none of the available tools can satisfy the question (e.g. user asks for live data we have no live source for) — without it, the model is stuck oscillating between tool selections under `allow_final=false` until loop-detect or MAX_ROUNDS forces an exit.

`ForcedAction` enum gains a `Refuse { reason }` variant; `parse_forced_action` adds the case; the loop driver maps `Refuse` to `content = format!("REFUSED: {reason}")` and `break`.

### §3.6 Tier F — `allow_final` scheduling pinned

Existing logic at `bridge.rs:840`:
```rust
let allow_final = !all_mcp_tool_calls.is_empty();
```

Codify this in a named helper + invariant test:

```rust
/// Round 0 must force a tool call; once any tool result lands, the model
/// gains the option to terminate via `final` or `refuse`.
fn allow_final_for_round(prior_tool_calls: usize) -> bool {
    prior_tool_calls > 0
}
```

Keeps allow_final=false when no tool result yet → the model has no logit space for disclaimer-as-final.

### §3.7 Streaming UX trade-off (acknowledged regression)

Forced-JSON path emits one structural JSON object per round; only the `final.answer` field's body is end-user text. Current `run_loop_forced_json` does not stream tokens (`sse_tap_tx: None` at `bridge.rs:896`). Native-path callers (Qwen3-Coder, Llama 3.1) currently get token-by-token streaming on the final text round.

**Decision**: accept the regression in this SDD. Final answer arrives as one chunk after loop completion. Mitigation:
1. Most MCP answers are <8KB → arrival latency dominated by inference time (5–30s typical), not by chunked streaming.
2. SSE `KeepAlive` heartbeats every 15s prevent perceived stall.
3. Future SDD `mcp-final-answer-token-streaming.md` may add partial-JSON streaming of the `final.answer` field (recognise the `"answer":"` prefix as it arrives, forward subsequent tokens until matching close-quote).

The user-visible delta on conv_33Af-class queries is +0–5 s perceived wait; on previously-failing conversations it's +∞ → finite (going from broken to working). Net positive.

### §3.8 Why this is "complete"

| Concern | How addressed |
|---|---|
| Disclaimer-after-tools (conv_33Af Turn 1/3) | `allow_final=true` schema's `final.answer` string is generated under grammar mask; the model cannot hide in non-JSON prose. The `refuse` branch is the only structurally-valid escape and carries an explicit reason (auditable). |
| Zero-round disclaimer (conv_33Af Turn 2/4) | `allow_final=false` on round 0 → no `final` branch in the schema → model logit-masked into a tool branch. |
| Native-path gap | Eliminated by deletion. |
| S23/S24 maintenance burden | Deleted. |
| Cross-model maintenance (`heuristic_supports_native` allow-list) | Eliminated for MCP. New Ollama models work from day 0. |
| Native customer tools (non-MCP `tools[]` in chat completions) | Untouched — `should_intercept()` returns false → original native dispatch in `chat_completions`. |
| OpenAI-compat passthrough | Untouched on the non-MCP path. |
| ACL / cap_points / top_k | Identical — applied before path dispatch. |
| Per-server timeout / circuit breaker / result cache | Identical — `execute_calls` unchanged. |
| S3 single-writer policy + turn_count bump | Already implemented in `run_loop_forced_json`; survives unification. |
| ClickHouse `mcp_tool_calls` ingest | `fire_mcp_ingest` survives unchanged. |

---

## §4 Files

| File | Change |
|---|---|
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | Drop heuristic gate; promote `run_loop_forced_json` → `run_loop`; delete legacy native loop, S23, S24, extract_tool_results, build_synthesis_messages |
| `crates/veronex/src/infrastructure/outbound/mcp/forced_json.rs` | Add `Refuse` variant to `ForcedAction`; add refuse branch to `build_forced_json_schema` (gated by allow_final); update `parse_forced_action`; extend tests |
| `crates/veronex/src/infrastructure/outbound/ollama/capability.rs` | Confirm `heuristic_supports_native` is unused after MCP unification (otherwise leave for non-MCP callers); update doc-comment |
| `docs/llm/inference/mcp.md` | Rewrite Architecture section (single path); strip S23 + S24 protections rows; document `Refuse` branch + `allow_final` scheduling |
| `docs/llm/flows/mcp.md` | Rewrite `run_loop()` ASCII diagram; strip convergence-boundary + synthesis-round flow steps; document refuse exit |
| `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` | Add status banner: superseded by this SDD; move to `history/` |
| `.specs/veronex/mcp-synthesis-round.md` | Add status banner: superseded by this SDD; move to `history/` |

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | `build_forced_json_schema(tools, allow_final=true)` includes 3 branch types (tool, final, refuse) | forced_json unit |
| 2 | `build_forced_json_schema(tools, allow_final=false)` includes only tool branches | forced_json unit (existing, keep) |
| 3 | `parse_forced_action` handles `{"action":"refuse","reason":"..."}` → `Refuse` | forced_json unit |
| 4 | `allow_final_for_round(0) == false`; `allow_final_for_round(1) == true` | bridge unit |
| 5 | Regression: conv_33Af Turn 1 reproducer — model gets `web_search` results → must emit `final.answer` non-empty (cannot disclaim) | bridge integration (mock model) |
| 6 | Regression: conv_33Af Turn 2 reproducer — round 0 with no tool results → schema rejects `final` branch (validate produced JSON shape, not model) | forced_json unit |
| 7 | Regression: model emits `refuse` → loop exits with `content = "REFUSED: ..."`, S3 turn carries the refusal | bridge integration |
| 8 | All existing forced_json tests pass unchanged (parse_tool_action, parse_final_action, fail-open paths, schema_to_response_format, render_tool_catalogue, system prompt assembly) | forced_json unit |
| 9 | Heuristic_supports_native is no longer reachable from `bridge.rs::run_loop` — grep assertion in code-review | meta |

---

## §6 Live verification (dev cluster)

### §6.1 Setup
- Deploy unified bridge to `develop-<this PR sha>`
- `qwen3-coder-next-200k:latest` (was failing native path)
- `llama3.1:8b-instruct-q4_K_M` (was failing native path — second model class)
- `qwen3:8b` (already on forced-JSON — regression check)
- MCP servers: web_search + get_weather + datetime active

### §6.2 PASS conditions

| # | Check | Expected |
|---|-------|----------|
| L1 | Submit conv_33Af Turn 1 verbatim ("뉴스 정보를 분석하여 오늘 미국 주식장에 들어가야할 섹터를 정리해서줘") | Final answer cites at least one tool result; **no disclaimer string** ("실시간 데이터 접근 권한 없" / "비활성화" / "I don't have access to real-time data") |
| L2 | Submit conv_33Af Turn 2 verbatim ("그러면 이제 추천 종목 코드와 목표가를 상세히 분석해줘") | Round 0 emits a tool branch; conversation has `tool_calls.length ≥ 1` for this turn |
| L3 | Submit conv_33Af Turn 4 verbatim ("아니 웹검색이 가능하잖아 헛소리할래?") | Round 0 emits a tool branch; not a refuse, not a disclaimer |
| L4 | Submit a non-MCP-suitable question ("내가 일주일 전에 뭐 했지?") with no datetime context | Model emits `refuse` with reason mentioning lack of personal-history tool; UI renders as REFUSED bubble |
| L5 | Latency: conv_33Af Turn 1 end-to-end ≤ 90 s on warm model | Within budget |
| L6 | qwen3:8b regression: same 4-turn conversation behaves identically before/after (was already on forced-JSON) | byte-identical S3 record except for trailing timestamps |
| L7 | Bridge log: zero occurrences of `MCP convergence`, `MCP synthesis round`, `extract_tool_results` after deploy | Deletion verified live |
| L8 | JSON validity: every model response in turn-internals dashboard parses as a `ForcedAction` (no fail-open path triggered) | Sample 50 turns; expect 50/50 |

### §6.3 ROLLBACK conditions

| Trigger | Action |
|---|--------|
| Any L1–L4 regresses to disclaimer / zero-round | Roll back to `develop-bf41f96` (last known good); reopen SDD with failure data |
| L8 < 95 % | Investigate whether older Ollama version is bypassing `format` enforcement; tighten capability detection |
| Latency p95 > 2× pre-deploy on conv_33Af-class | Inspect: forced-JSON `Final` round may need to skip the schema for plain-text emit (future-work hook) |

---

## §7 CDD sync (post-impl)

| File | Action |
|---|---|
| `docs/llm/inference/mcp.md` | Rewrite "Architecture" diagram to show single `run_loop` (constrained-decoding); rewrite "Protections" table to drop S23 + S24 rows, add `refuse` branch row; update "Why this honors the gateway promise" to reflect universal application; remove "Capability gate" subsection (no longer routes by model family) |
| `docs/llm/flows/mcp.md` | Rewrite `run_loop()` ASCII to a single linear flow without the synthesis-fallback branch and without the convergence-boundary check; update "Loop Protections" table |
| `docs/llm/policies/architecture.md` | If the AppState wiring documents the bridge dual-path, update to single-path |

---

## §8 Out of Scope

- **Streaming `final.answer` token-by-token** — separate future SDD `mcp-final-answer-token-streaming.md`. Current scope ships single-chunk delivery (acceptable per §3.7).
- **Removing `heuristic_supports_native` entirely** — non-MCP code paths may still reference it; leave intact, just unreferenced from MCP.
- **Renaming files** — `run_loop_forced_json` is renamed in-function but the file stays `bridge.rs` (no module relocation).
- **Per-tool capability hints** — currently `forced_json` system prompt is generic across all tools. Per-tool hints are a follow-up if observed quality issues with multi-tool catalogs.
- **MCP for Gemini provider** — Gemini still uses native `function_call` API; this SDD covers Ollama dispatch only. The `should_intercept()` gate restricts to Ollama already (`provider_type == "ollama"` check upstream).
- **Replacing `MAX_ROUNDS=5`** — keep at 5 as a safety net (loop-detect threshold is the primary control).

---

## §9 References

- CDD `docs/llm/inference/mcp.md` §"Why this honors the gateway promise"
- CDD `docs/llm/flows/mcp.md` §"run_loop"
- `crates/veronex/src/infrastructure/outbound/mcp/forced_json.rs` — existing constrained-decoding implementation
- `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` — dual-path source
- `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` — S23 (superseded)
- `.specs/veronex/mcp-synthesis-round.md` — S24 (superseded)
- conv_33AfPaddqdXSiqIHX081T — live failure reproducer (4 turns, all failed gateway contract)
- [QwenLM/Qwen3-Coder #475](https://github.com/QwenLM/Qwen3-Coder/issues/475) — history-mimicry pattern (root cause of S23/S24)
- [Ollama #8421](https://github.com/ollama/ollama/issues/8421), [#11171](https://github.com/ollama/ollama/issues/11171) — `tool_choice` ignored (root cause of S23 schema-removal hack)
- [llama.cpp grammar-constrained generation](https://github.com/ggerganov/llama.cpp/blob/master/grammars/README.md) — the underlying primitive
