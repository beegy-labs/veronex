# SDD: Bridge MCP loop — Drop streaming fast-path + Stream-tap

> Status: planned | Change type: **Fix** (correctness — eliminates round-level MCP detection bypass) + **structural** (token-tap pattern)
> Created: 2026-04-30 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/policies/architecture.md` (Hexagonal — invariants enforced structurally) · `.specs/veronex/history/inference-mcp-streaming-first.md` (premise invalidated, see §2)
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S20 (to add)
> **Resume rule**: every section is self-contained. Future session reading this SDD alone (no chat history) must be able to continue from the last unchecked box.

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — Drop fast-path in `run_loop` (`bridge.rs:354–363`) | [ ] pending | — | — |
| B — Add optional `sse_tx` tap to `collect_round` | [ ] pending | — | — |
| C — Wire tap from `run_loop` → `mcp_ollama_chat` SSE writer | [ ] pending | — | — |
| D — Tests (round-level invariants + tap-mode invariants) | [ ] pending | — | — |
| CDD-sync (`mcp.md` MCP loop section + streaming-first history note) | [ ] pending | — | — |
| Live verify (dev) — 200K MCP cold-load, multi-round, token streaming preserved | [ ] pending | — | — |

If you find this SDD with all boxes unchecked, start at §A. If A is checked, start at §B. Etc.

---

## §1 Problem (verified 2026-04-30 on dev `develop-1076325`)

S19.1 live verify (`SK하이닉스 1분기 분석`) on `qwen3-coder-next-200k:latest`:

```
04:26:32  request submitted (200K-context, MCP web_search available)
04:30:09  round 0 lifecycle.ensure_ready outcome=LoadCompleted duration_ms=216841   ✓ S19.1 timeout fix verified
04:30:15  bridge: "MCP round complete round=0 mcp_calls=1"                          ✓ Phase 2 first-token within 300s budget
04:30:16  round 1 lifecycle.ensure_ready outcome=AlreadyLoaded duration_ms=0
04:30:21  HTTP response ends — SSE stream contains tool_call + finish_reason=stop
          *** NO round=1 mcp_calls log line ***
```

The bridge logged round 0 completing with one MCP tool call. The MCP tool was executed and its result was injected into round 1's prompt. But **round 1 was never collected by the bridge**. Round 1's job emitted ANOTHER tool_call (same query) which the client received raw via SSE and could not act on.

Traced to the streaming fast-path at `bridge.rs:354–363`:

```rust
} else if want_stream && rounds > 0 {
    // Streaming fast-path: at least one MCP tool-call round completed,
    // so this next job is almost certainly the final text response.
    final_job_id = Some(job_id);
    break;
}
```

The comment "almost certainly the final text response" is an unverified assumption. When it fails (model emits another tool_call instead of text), the bridge has already committed to streaming round 1 directly to the client without inspection — bypassing MCP detection, loop detection, and result persistence.

---

## §2 Root cause — premise of streaming-first SDD invalidated

The fast-path was introduced in `feat/mcp-streaming-first` (PR #102, SDD `.specs/veronex/history/inference-mcp-streaming-first.md`). That SDD's §1 stated the SOLE driving constraint:

> "Cloudflare's origin idle-read timeout is **~100 s** on the standard plan; the bridge's blocking response cannot squeeze under that without artificial truncation."

The fast-path was the optimization to ensure final-round tokens stream to the client immediately, bounding any single SSE-idle window under CF Edge's 100 s.

**Platform-gitops PRs #598/#599/#600 (merged 2026-04-30) introduced CF-bypass routing** for the inference path:
- `veronex-api-dev.girok.dev` (DNS-only CNAME → `home-gw.girok.dev`) lands directly on `cilium-gateway-web-gateway`
- Cilium HTTPRoute `timeouts.{request,backendRequest}: 1800s` per `cilium-gateway-values.yaml#veronex-api-direct-dev-route`
- CF Edge / CF Tunnel removed from the inference path

The fast-path's foundational reason **no longer exists**. It now stands as an unverified assumption (round N+1 ⇒ text) without compensating necessity.

### Industry alignment (verified 2026-04-30 web search)

| Framework | Round-level handling | Token forwarding |
|-----------|---------------------|------------------|
| LangGraph `astream` | Synchronous per-step collection | Token-by-token within step (per stream mode) |
| OpenAI Agents SDK v0.12.x | Synchronous per-step + tool exec | `RawResponsesStreamEvent` deltas forwarded |
| vLLM | (inference layer) inline tool_call parsing | Each delta emitted as generated |
| OpenAI streaming spec | tool_calls / content single-round XOR | Per-token deltas |

**No production framework uses a round-bypass fast-path.** All run rounds synchronously while forwarding tokens within each round. Veronex's current fast-path is non-standard.

### CDD alignment

`docs/llm/policies/architecture.md` (Constitutional): "invariants must hold structurally". MCP loop's invariant ("each round's tool_calls inspected, executed, and fed back") is currently maintained by code path coincidence, not structurally — the fast-path branch silently breaks it. Drop the bypass and the invariant becomes structural again.

---

## §3 Solution

### §3.1 Two parts of one fix

| Part | What | Why |
|------|------|-----|
| **Drop fast-path** | Remove `bridge.rs:354–363` branch. All rounds go through `collect_round` synchronously. | Restores MCP loop invariant. Loop detection works. Result_text always populated. |
| **Stream-tap** | Modify `collect_round` to optionally forward token chunks to caller's SSE writer as they arrive. Caller passes a `tokio::sync::mpsc::UnboundedSender<TokenChunk>` if streaming is desired. | Preserves chatGPT-style token-by-token UX for final-round text without compromising correctness. Aligns with LangGraph / OpenAI Agents SDK pattern. |

These two parts MUST land together. Drop without tap loses token streaming UX (regression). Tap without drop is half a fix.

### §3.2 Stream-tap rule (OpenAI spec — round-level XOR)

Within a single round, the model emits **either** text content **or** tool_calls — not both (mixed delta is bug territory; see vLLM bug #36435/#40816). The tap exploits this:

```text
round_state = "undecided"

for token in stream:
    accumulate(token)  // existing — for round_result

    if sse_tx is Some:
        if round_state == "undecided":
            if token has tool_calls: round_state = "intercept"   // never forward
            elif token has content:   round_state = "passthrough" // forward this and all subsequent
            else:                     continue (heartbeat / phase_boundary)

        if round_state == "passthrough":
            sse_tx.send(token_chunk)   // forward to caller
        # else intercept: silent
```

After `collect_round` returns, the bridge inspects `RoundResult`:
- If `tool_calls` non-empty AND any are MCP-prefixed → execute tools, continue loop (tap was silent for this round, client saw nothing)
- If `tool_calls` non-empty AND none MCP → break, return tool_calls to client (tap was silent; caller emits as final chunk)
- If pure text → break, content already streamed via tap (caller emits only `[DONE]`)

### §3.3 Mixed-delta safety (vLLM bug class)

If a malformed model emits content first then tool_calls in same round:
- Tap entered `passthrough` mode on first content token → forwarded text to client
- Bridge later detects tool_calls in RoundResult
- **Decision**: treat the round as text-final (already streamed). MCP tool_calls in malformed mixed-delta rounds are dropped silently with a `warn!` log. Justification: client already received "final" SSE; introducing an MCP execution mid-stream creates an inconsistent client experience worse than dropping the malformed tool_call.

This is documented behavior, not a regression — current code on the same path would also miss the MCP tool_call (it's a model-side spec violation).

---

## §4 Files

| File | Change |
|---|---|
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | (a) Remove `run_loop` fast-path branch (lines 354–363). (b) Modify `collect_round` signature: add `sse_tx: Option<UnboundedSender<TokenChunk>>`. (c) Inside `collect_round` token loop, implement passthrough/intercept logic per §3.2. (d) `run_loop` creates mpsc channel, passes sender to each round's `collect_round`. (e) `McpLoopResult.final_job_id` field becomes dead — remove. |
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | (a) `mcp_ollama_chat`: create mpsc channel `(tap_tx, tap_rx)`. (b) Pass `tap_tx` into `bridge.run_loop`. (c) SSE stream consumes from `tap_rx` AND awaits bridge's oneshot. Channel events emit as SSE chunks; bridge oneshot signals end-of-loop with summary (finish_reason, usage). (d) Remove `final_job_id` streaming branch (lines 845–898). (e) `loop_result.content` short-circuit becomes the "tap was silent" fallback (no tap forward happened — emit content as final chunk). |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` (tests) | New tests per §5. |

### §4.1 `TokenChunk` shape

The tap channel carries OpenAI-compat `delta` chunks ready to be wrapped as SSE events:

```rust
pub struct TokenChunk {
    pub kind: TokenChunkKind,
}

pub enum TokenChunkKind {
    Content(String),                  // delta.content
    Done { finish_reason: String, usage: Option<UsageInfo> },  // round end
}
```

Tool_call chunks are NOT carried by the tap (intercept mode never forwards). Final round's tool_calls (non-MCP) are returned via `RoundResult` and emitted by the caller as the SSE wraps up.

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | `run_loop` round 0 tool_call → round 1 always synchronously collected (no fast-path) — mock looping model emits same tool_call 3× → `LOOP_DETECT_THRESHOLD=3` fires, bridge breaks with warn log | bridge unit |
| 2 | `run_loop` round 0 final text (no tool_call) → tap forwards content tokens, bridge breaks on `mcp_calls.is_empty()` | bridge unit |
| 3 | `run_loop` round 0 tool_call → round 1 final text → tap silent on round 0, forwarding on round 1 | bridge unit |
| 4 | `collect_round` with `sse_tx=None` (legacy callers) → no behavior change vs pre-S20 | bridge unit |
| 5 | `collect_round` with `sse_tx=Some` and tool_call first → no chunks sent on channel; `RoundResult.tool_calls` populated | bridge unit |
| 6 | `collect_round` with `sse_tx=Some` and content first → all subsequent content chunks sent on channel | bridge unit |
| 7 | `collect_round` mixed-delta (content first, tool_call later — vLLM bug class) → passthrough mode wins, tool_call dropped from intercept; warn log emitted | bridge unit |
| 8 | All existing phase-aware timeout invariants (S19/S19.1) unchanged | bridge unit |

---

## §6 Live verification (dev cluster)

### §6.1 Setup
- Image rolled out to `develop-<S20-sha>` (post-merge)
- `MCP_LIFECYCLE_PHASE=on` (already)
- CF-bypass route `veronex-api-dev.girok.dev` (already)

### §6.2 Scenario A — looping model converges via final text

Same prompt as S19.1 verify (`오늘 SK하이닉스 1분기 실적 분석해줘`) on `qwen3-coder-next-200k:latest`. Model is now expected to:
- Round 0: cold-load + emit `web_search` tool_call (interceptable) — bridge collects
- Round 1+: model uses tool result to produce final text — bridge collects, tap forwards tokens

Verify:
- [ ] No `model_hung_post_load` (S19.1 timeout fix still holds)
- [ ] Bridge log: `round=0 mcp_calls=1` AND `round=1 mcp_calls=0` (or higher rounds if model loops gracefully)
- [ ] HTTP response: SSE stream emits `delta.content` chunks token-by-token during final round
- [ ] Final SSE chunk has `finish_reason: stop` (not `tool_calls`)
- [ ] Conversation `result_text` non-empty (`/v1/dashboard/conversations/{id}` returns turn with text)

### §6.3 Scenario B — looping model hits LOOP_DETECT

If the model insists on calling the same tool 3+ times:
- Bridge log: `LOOP_DETECT — breaking` warn at round 2 (third call)
- HTTP response: SSE error event with clear loop_detected message + `[DONE]`
- Conversation has partial result with loop_detected indicator

This is an EXPLICIT failure mode (clean error code) replacing the prior SILENT failure (orphan tool_call streamed to client).

### §6.4 Scenario C — non-MCP simple chat

Sanity check: prompt that doesn't trigger tools (`안녕`) → model emits text directly in round 0 → tap forwards → conversation has clean result. Confirms no regression on non-tool path.

---

## §7 CDD sync (post-impl)

Per `.add/doc-sync.md`. Branch: `docs/cdd-mcp-loop-correctness` (or same PR if scope small).

| File | Action |
|---|---|
| `docs/llm/inference/mcp.md` | Update "Architecture" diagram + "Phase-aware timing" row (already has S19/S19.1). Add subsection: "Round-level synchronous collection — no fast-path. Token-tap preserves UX." Replace any text implying "round N+1 streams directly" with "round N+1 collected synchronously, tokens forwarded via tap". |
| `.specs/veronex/history/inference-mcp-streaming-first.md` | Append §10 footnote: "Premise §1 (CF Edge 100s) invalidated by platform-gitops PRs #598/#599/#600 (CF-bypass for inference). Fast-path optimization removed in S20 (`bridge-mcp-loop-correctness.md`); SSE framing + KeepAlive heartbeat retained." |
| `.specs/veronex/bridge-mcp-loop-correctness.md` | Mark §0 boxes [x] as work proceeds; archive to `.specs/veronex/history/` after live verify pass. |
| `.specs/veronex/history/scopes/2026-Q2.md` | Add S20 row referencing this SDD. |

---

## §8 Out of Scope

- Token streaming for non-MCP route (`stream:true` to `/v1/chat/completions` without MCP) — already works via direct ollama path, untouched
- ReAct shim path (`run_loop_react`) — has its own structure; not subject to this fix. Future SDD if symmetric issue surfaces
- Loop-detection threshold tuning — `LOOP_DETECT_THRESHOLD=3` retained; revisit if production rate exceeds expectation

---

## §9 References

- `.specs/veronex/history/inference-mcp-streaming-first.md` (PR #102) — origin of fast-path
- `.specs/veronex/history/inference-lifecycle-sod.md` (S14) — Phase 1/2 split (pre-requisite for phase-aware timing)
- `.specs/veronex/bridge-phase-aware-timing.md` (S19/S19.1) — phase-aware timeouts (independent fix)
- platform-gitops PR #598 (Cilium HTTPRoute), #599 (cloudflare-ddns staticRecords), #600 (web direct routes), #601 (CDD layer-boundary clarification) — CF-bypass infra that invalidates streaming-first §1 premise
- vLLM bug [#36435](https://github.com/vllm-project/vllm/issues/36435), [#40816](https://github.com/vllm-project/vllm/issues/40816) — mixed-delta bug class informing §3.3 safety
- LangGraph streaming docs (DeepWiki) — synchronous per-step + per-token forward pattern
- OpenAI Agents SDK streaming docs — `RawResponsesStreamEvent` per-token forwarding
