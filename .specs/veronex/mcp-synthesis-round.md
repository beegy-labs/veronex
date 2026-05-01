# SDD: MCP synthesis round (degenerate-loop final guarantee)

> Status: planned | Change type: **Feature** (forced-text safety net) | Created: 2026-04-30 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/flows/mcp.md` · `docs/llm/inference/job-lifecycle.md`
> Predecessor: `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` (S23) — convergence boundary + tools-omission

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — `extract_tool_results` helper: walk messages, collect `role:"tool"` entries, format as plain text | [x] | #129 | dc540c9 |
| B — `synthesis_round` helper: fresh `[system, user, system-with-results]` messages, `tools: None`, dispatch via `state.use_case.submit` + `collect_round` (token streaming via `sse_tap_tx`) | [x] | #129 | dc540c9 |
| C — `run_loop` integration: after the for-loop, if `content.is_empty()` and `rounds > 0`, invoke synthesis; replace returned `content` with synthesis output | [x] | #129 | dc540c9 |
| D — Tests: helper unit tests + invariant pin (synthesis fires iff degenerate after S23 boundary) | [x] | #129 | dc540c9 |
| CDD-sync — `mcp.md` (synthesis round subsection) + `flows/mcp.md` (run_loop diagram + protections row) | [x] | #129 | dc540c9 |
| Live verify — L1/L2/L3/L4 — synthesis dispatched, succeeded, 3115-char text content persisted to S3 as 6th turn | [x] (live-verified 2026-05-01) | n/a | conv_338rqmFdyh9mGUVQbqiNU |
| L5 follow-up — `SSE_TIMEOUT=300s` was tripping mid-stream on long synth runs; bumped to 1700s (under Cilium 1800s) | [x] | TBD | TBD |

---

## §1 Problem (verified 2026-04-30 dev `develop-326c66a`)

S23 v3 (PR #128) closed the schema-leak gap on the convergence boundary —
the final round on a degenerate run is now dispatched with `tools: None`,
verified at `bridge.rs::run_loop` and confirmed clean at the Ollama
adapter (`adapter.rs:589` skips `body["tools"]` when None).

Live re-test on dev with the user's reproducer prompt (5-round
degenerate-trigger) showed:

```
16:27:05.655837  MCP convergence: tools omitted + system message injected  round=4 max_rounds=5
16:27:14.389823  MCP round complete  round=4  mcp_calls=1
```

The boundary fired correctly; tools were stripped. **The model still
emitted a tool_call** — `mcp_veronex_mcp_dev_verobee_com_web_search`
with valid args — because Qwen3-Coder learned the function name from
prior `assistant.tool_calls` entries left in the message history (kept
intact by `prune_tool_messages` per OpenAI spec for `tool_call_id` ↔
`tool` result pairing) and reproduced the pattern from training even
without tool schemas. Documented behaviour:
[QwenLM/Qwen3-Coder #475](https://github.com/QwenLM/Qwen3-Coder/issues/475).

The round-loop's `mcp_calls.is_empty()` break never triggers; the loop
exits at `max_rounds`; `content` stays empty; UI shows "(저장된 결과
없음)" repeated for every round. Test-panel inference returns no answer
to the user.

S23 boundary alone is insufficient because the degenerate signal
(history of prior tool_calls) lives in the SAME messages array we
re-submit to the model.

---

## §2 Root cause

The model's tool-calling behaviour is conditioned by **two** signals:
the `tools` schema (which v3 already removes on boundary) and the
**conversation history of prior `assistant.tool_calls` entries**
(which v3 still leaves intact). With history pattern matching alone,
the model can fabricate a syntactically correct `<tool_call>` token
sequence; the Ollama qwen3_coder parser then promotes it to a structured
`tool_calls` JSON in the response.

Removing prior `assistant.tool_calls` from the live history would break
the OpenAI spec invariant (each `role:"tool"` message must reference an
existing `tool_call_id` from a preceding assistant message). So we
cannot just delete them in-place.

The only correct fix at the gateway layer is to **dispatch a separate
synthesis round on a fresh messages array** that:
- contains only the user's original prompt and tool **results** (not
  tool_calls),
- carries no `tools` schema,
- carries no prior assistant tool_call entries.

The model in this synthesis round has nothing to mimic, no schemas to
follow, and an explicit text-only directive. It must emit text or
trivially nothing.

---

## §3 Solution

### §3.1 Tier A — `extract_tool_results` helper

```rust
/// Walk a messages array and collect the textual content of every
/// `role:"tool"` entry, in order, into a single string suitable for
/// injection as a synthesis-round system message.
///
/// Output format:
///   "Search/tool result 1: <content>\n\nSearch/tool result 2: ..."
///
/// Returns `None` if no tool messages were found (caller skips synthesis).
fn extract_tool_results(messages: &[Value]) -> Option<String> { ... }
```

Pure function; no I/O; pinned by unit tests.

### §3.2 Tier B — `synthesis_round` helper

```rust
async fn synthesis_round(
    state: &AppState,
    caller: &Caller,
    model: &str,
    original_prompt: &str,
    tool_results_text: &str,
    sse_tap_tx: Option<&UnboundedSender<String>>,
    mcp_loop_id: Uuid,
    conversation_id: Option<ConversationId>,
) -> Result<RoundResult, RoundError> {
    // Layered date anchor: the synthesis round builds a FRESH messages
    // array, so the original `inject_current_datetime` system message at
    // messages[0] of the user's request never reaches it. Re-injecting
    // here closes the drift gap that the convergence-boundary fix (#138)
    // could not — convergence reinforcement only fires on the in-loop
    // final round, not on this fallback dispatch.
    let date_anchor = build_current_datetime_system_text();
    let synth_messages = vec![
        serde_json::json!({"role": "system", "content": date_anchor}),
        serde_json::json!({
            "role": "system",
            "content": "You are answering the user's question. \
                Tools have already been used to gather the information \
                you need. Do NOT call any tools. Using the tool results \
                provided below, produce a complete, well-structured \
                answer to the user's question in their original language. \
                Honor the date constraints in the system message above — \
                every \"today\" / \"recent\" / \"현재\" / \"최근\" in your \
                response refers to the current date listed there, not to \
                your training cutoff."
        }),
        serde_json::json!({
            "role": "user",
            "content": original_prompt,
        }),
        serde_json::json!({
            "role": "system",
            "content": format!("Tool results gathered:\n\n{}", tool_results_text),
        }),
    ];

    let job_id = state.use_case.submit(SubmitJobRequest {
        prompt: original_prompt.to_string(),
        model_name: model.to_string(),
        provider_type: ProviderType::Ollama,
        messages: Some(Value::Array(synth_messages)),
        tools: None,                              // ← no schemas
        request_path: Some("/v1/chat/completions".to_string()),
        mcp_loop_id: Some(mcp_loop_id),
        // ... other fields default
    }).await?;

    collect_round(state, &job_id, sse_tap_tx).await
}
```

Streams via `sse_tap_tx` so the user sees incremental output in the
test panel. Persisted as a normal `TurnRecord` in S3 keyed by the
synthesis job_id.

### §3.3 Tier C — `run_loop` integration

```rust
for round in 0..max_rounds { /* unchanged */ }

// ── S24 synthesis fallback ─────────────────────────────────────────
// If the loop exited at max_rounds with no text content and at least
// one tool round executed (so we have results to synthesize from),
// dispatch a synthesis round on a fresh messages array. This is the
// final safety net: the model cannot mimic prior tool_calls because
// the synth messages array contains none. SDD: §3.
if content.is_empty() && rounds > 0 {
    if let Some(results) = extract_tool_results(&messages) {
        info!(rounds, "MCP synthesis round: dispatching forced-text fallback");
        let original_prompt = extract_last_user_prompt(&messages);
        match synthesis_round(state, caller, &model, &original_prompt, &results,
                              sse_tap_tx.as_ref(), mcp_loop_id, conversation_id).await {
            Ok(r) => {
                total_prompt_tokens = total_prompt_tokens.saturating_add(r.prompt_tokens);
                total_completion_tokens = total_completion_tokens.saturating_add(r.completion_tokens);
                if r.passthrough_streamed {
                    streamed_via_tap = true;
                }
                content = r.content;
                finish_reason = r.finish_reason;
            }
            Err(e) => {
                warn!(error = %e, "MCP synthesis round failed — surfacing degenerate result");
            }
        }
    }
}
```

Failure-mode: if synthesis itself errors (timeout, provider down), the
bridge falls through to the existing degenerate-return path (caller
sees the full audit chain via Tier A of S23). No worse than today.

### §3.4 Why this is "complete"

| Concern | How addressed |
|---|---|
| Model mimics prior tool_calls | Synthesis round has none in its messages array |
| Tools schema leak | Synthesis round explicitly `tools: None` (Ollama-correct per S23) |
| User loses the tool chain | S3 stores synthesis as one extra TurnRecord; PG audit (S23 Tier A) still has every round of the original loop |
| Cost | One extra inference call ONLY when degenerate (rare path); typical 2-turn natural-convergence runs are unaffected |
| Latency | Adds ≤ ROUND_TOTAL_TIMEOUT (1500s cap, but typical text generation is 5–30s); acceptable for a guaranteed-answer mode |
| Token streaming | Synthesis uses `sse_tap_tx` → user sees text stream live |

---

## §4 Files

| File | Change |
|---|---|
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | New `extract_tool_results` + `synthesis_round` helpers; `run_loop` post-loop synthesis dispatch |
| `docs/llm/inference/mcp.md` | New "Synthesis round" subsection; protections table row |
| `docs/llm/flows/mcp.md` | Add post-loop synthesis step in `run_loop` diagram + protections row |

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | `extract_tool_results` returns None when no `role:"tool"` entries present | bridge unit |
| 2 | `extract_tool_results` returns concatenated text when N tool entries present | bridge unit |
| 3 | Synthesis predicate: dispatch iff `content.is_empty() && rounds > 0 && tool_results.is_some()` | bridge unit |
| 4 | Synthesis messages shape: 3 entries, no tool_calls, no tools schema | bridge unit |
| 5 | (regression) all existing 37 bridge tests pass unchanged | bridge unit |

---

## §6 Live verification (dev cluster)

### §6.1 Setup
- Image rolled out to `develop-<this PR sha>`
- `qwen3-coder-next-200k:latest` warm

### §6.2 PASS conditions

| # | Check |
|---|---|
| L1 | Submit the user's reproducer prompt ("오늘 마이크론 주가에 대해 알려주고 전망에 대해서도 조사해줘 최근 메모리반도체 상승과 마이크론과 삼성 하이닉스의 우위를 비교해줘") with `use_mcp:true` |
| L2 | Bridge log: `MCP synthesis round: dispatching forced-text fallback` line emitted exactly once after `round=4` complete |
| L3 | Final SSE response contains text content (`delta.content` chunks); `finish_reason=stop` |
| L4 | S3 ConversationRecord includes one extra TurnRecord with `result_text` non-empty (the synthesis round's answer) |
| L5 | UI test panel renders the assistant bubble with the synthesized text answer (not "(저장된 결과 없음)") |

---

## §7 CDD sync (post-impl)

| File | Action |
|---|---|
| `docs/llm/inference/mcp.md` | Append "Synthesis round" subsection in Audit/Loop section. Add a Protections-table row. |
| `docs/llm/flows/mcp.md` | Add post-loop synthesis branch in the `run_loop()` ASCII diagram. Add a Loop Protections row. |

---

## §8 Out of Scope

- Multi-model synthesis (using a different model for the synthesis round) — single-model is enough; if user wants better quality on that one round, separate model-routing SDD.
- Tool-result truncation logic for very long accumulated results — current `MAX_TOOL_RESULT_BYTES=32768` per call × up to 5 rounds = 160KB worst case, well under 200K context. Revisit only if observed.
- Auto-retry on synthesis failure — single attempt is fine; if it fails the user sees the existing degenerate-fallback (audit chain), no regression.
- Stripping prior assistant tool_calls from the live history (rejected — breaks OpenAI tool_call_id ↔ tool result invariant).

---

## §9 References

- `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` — S23 (this builds on)
- `docs/llm/inference/mcp.md` — MCP integration overview
- `docs/llm/flows/mcp.md` — `run_loop` ASCII flow
- [QwenLM/Qwen3-Coder #475](https://github.com/QwenLM/Qwen3-Coder/issues/475) — degenerate tool-call loops on history-only signal
- [Ollama #8421](https://github.com/ollama/ollama/issues/8421) / [#11171](https://github.com/ollama/ollama/issues/11171) — `tool_choice` not supported (S23 motivation)
