# SDD: Inference MCP — Streaming-First + Cancel-Resilient Persistence

> Status: planned | Change type: **Change** (entry-handler routing) + **Add** (cancel-resilient finalize) | Created: 2026-04-28 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/inference/job-lifecycle.md` · `docs/llm/inference/job-api.md` · `docs/llm/flows/mcp.md` · `docs/llm/policies/architecture.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S15
> **Resume rule**: every section is self-contained — any future session reading this SDD alone (no chat history) must continue from the last unchecked box.

---

## §0 Quick-resume State

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| A — MCP entry forces SSE streaming + heartbeat | [ ] not started | `feat/mcp-streaming-first` (TBD) | — | — |
| B — Cancel-resilient finalize_job (always write S3) | [ ] not started | `fix/finalize-on-cancel` (TBD) | — | — |
| C — Test panel UI: stream-first + tool-call progress | [ ] not started | `feat/test-panel-stream` (TBD, web/) | — | — |
| CDD-sync (post C) | [ ] blocked on A+B | `docs/cdd-mcp-streaming` (TBD) | — | — |
| Live verify (dev) | [ ] blocked on CDD-sync | dev cluster + test-3 user | — | — |

---

## §1 Problem (verified live, 2026-04-28)

Test panel submits `/v1/chat/completions` with `stream:false` for the 200K-context model + MCP tools enabled. User-visible symptoms: "load failed" / "저장된 결과 없음" / 524 from Cloudflare.

| # | Defect | Evidence |
|---|--------|----------|
| D1 | Cloudflare 524 at ~125s | `curl ... | head` ⇒ `HTTP/2 524` from `cf-ray` server |
| D2 | DB `inference_jobs.result_text` empty even when `has_tool_calls=true` and `completion_tokens > 0` | 6/6 recent test-source jobs: `result_len=0`, `tokens 47–83` |
| D3 | S3 ConversationRecord missing — no `result` written | finalize_job never runs on cancel path |
| D4 | MCP bridge `run_loop` cancels mid-loop on client disconnect | `CancelOnDrop` → `use_case.cancel(parent_job_id)` propagates to inner per-round jobs |

Lifecycle SoD (PR #91/#92/#93/#96) is unaffected — `lifecycle.ensure_ready outcome=AlreadyLoaded duration_ms=0` confirmed in dev logs. Phase 2 (token streaming + finalization) is the broken layer.

---

## §2 Root Cause

**Architectural mismatch between synchronous HTTP request-response and asynchronous agentic loop**:

1. `chat_completions` with `stream:false` blocks the HTTP socket until `bridge.run_loop` completes. For tool-using prompts (web_search, etc.) average round = ~30 s × up to MAX_ROUNDS (5). Total can hit 30–150 s, exceeding any HTTP intermediary's idle-read timeout.
2. Cloudflare's origin idle-read timeout is ~100 s on the standard plan; the bridge's blocking response cannot squeeze under that without artificial truncation.
3. Cloudflare 524 forces client disconnect → `CancelGuard::drop` (`cancel_guard.rs`) spawns `use_case.cancel(job_id)` → propagates to the round-job currently in `run_job` → `JobStatus::Cancelled` → `run_job` returns `Ok(None)` at the early-exit branch (`runner.rs:528`) **without invoking `finalize_job`**. S3 ConversationRecord is therefore never written.
4. UI fetches `/v1/dashboard/jobs/{id}` which reads from S3 (per `job-api.md`); a missing record renders as "(no result stored)" / "저장된 결과 없음".

The synchronous-bundle model is structurally incapable of absorbing an unbounded agentic loop's variance under any HTTP-tier deadline.

---

## §3 Solution

Two-axis fix; each axis is independent and can land separately. CDD storage architecture (S3 ConversationRecord per `inference/job-lifecycle.md`) is **preserved** — no new tables, no schema migrations.

### §3.1 Axis A — MCP forces SSE streaming response

> Goal: keep TCP/HTTP layer alive throughout the loop's variance window. Eliminate Cloudflare 524 entirely for MCP-routed requests.

`openai_handlers::chat_completions`: when `should_intercept()` returns `true`, **ignore the client's `stream:false` and respond with SSE** anyway. The body is composed of:

```
data: {"id":"...","choices":[{"delta":{"role":"assistant"}}]}\n\n
                                                                ← keep-alive heartbeat (every 15s)
:                                                              \n\n
                                                                ← intermediate event (per round)
event: mcp.tool_call
data: {"round":0,"tool":"web_search","args":{...}}\n\n

event: mcp.tool_result
data: {"round":0,"server":"...","bytes":2486}\n\n

                                                                ← final delta + done
data: {"id":"...","choices":[{"delta":{"content":"여기 마이크론 ..."}}]}\n\n
data: {"id":"...","choices":[{"finish_reason":"stop"}]}\n\n
data: [DONE]\n\n
```

Reasons (verified via web search):
- OpenAI Realtime/Responses APIs and Anthropic streaming all use SSE keep-alive for tool-using sessions; no major LLM API uses non-stream HTTP for agentic loops in production.
- SSE writes through Cloudflare (no buffering when `Content-Type: text/event-stream`).
- Backwards compatibility for clients expecting `stream:false`: the **final event includes the full bundled `chat.completion` message**; legacy clients that just await final body still work after re-aggregation. Modern clients can subscribe to intermediate events.

> **Decision: server-driven SSE for MCP regardless of client request.** Documented as a documented contract in `mcp.md` — clients sending `stream:false` to an MCP-enabled key receive an SSE response; non-MCP requests honour `stream:false`.

### §3.2 Axis B — Cancel-resilient finalize_job → always write S3

> Goal: any per-round inference_job that produced any tokens (including just `tool_calls`) must write its S3 ConversationRecord. The DB row's `has_tool_calls`/tokens fields are already populated via the normal path — we are bringing the S3 side of the same write into the cancel branch.

Current `runner.rs:520-530` cancel path (early exit):

```rust
if entry.status == JobStatus::Cancelled {
    drop(entry);
    decr_running(&valkey).await;
    return Ok(None);                     // ← S3 write skipped
}
```

After Tier B:

```rust
if entry.status == JobStatus::Cancelled {
    drop(entry);
    // Persist whatever we accumulated so far. The MCP bridge / UI can
    // still recover the partial conversation from S3 (per CDD
    // `inference/job-lifecycle.md` — S3 ConversationRecord is the SSOT
    // for `result`, `messages`, `tool_calls`).
    finalize_partial_on_cancel(
        &jobs, &mut job, &message_store, &valkey,
        ...,
        &ts,                              // tokens accumulated so far
        original_messages,
        original_prompt,
        uuid,
        started_at,
    ).await;
    return Ok(None);
}
```

`finalize_partial_on_cancel` is a new helper next to `finalize_job` in `runner.rs`. It performs the **S3 write only** (DB row's status is already `Cancelled`, no UPDATE needed beyond what `cancel()` already does). Token stream state — `ts.text`, `ts.tool_calls`, `ts.completion_tokens` — feed the same `ConversationRecord { prompt, messages, tool_calls, result }` shape; `result` is `(!ts.text.is_empty()).then_some(strip_think_blocks(ts.text))` (identical to non-cancel path).

Edge cases:
- Cancel before any token (e.g. queue cancel) → `ts.text.is_empty() && ts.tool_calls.is_empty()` → S3 write skipped (no useful record).
- Cancel mid-stream after model emitted partial tool_calls → S3 record stores the partial tool_calls; UI surfaces "Tool Calls" section per `job-api.md` rule (`result_text=None && tool_calls_json≠None`).
- Ownership-lost branch (line 245) already returns without finalize — same Tier B treatment applies symmetrically.

CDD invariant preserved: S3 is the SSOT for conversation content; DB row is metadata/index only. CDD update needed: clarify that S3 write is now **partial-on-cancel** path-aware, not "finalize-only".

### §3.3 Axis C — Test panel UI consumes the SSE

`web/app/api-test/...` (Test Run panel) currently uses fetch-with-`stream:false`. Replace with `EventSource` (or `fetch` + `getReader()` text-stream parser if SSE-with-cookies needed via cross-origin). Render:

| Event | UI |
|-------|-----|
| `mcp.tool_call` | Append "Tool: {name} ({server}) — running…" row |
| `mcp.tool_result` | Mark previous row "✓ {bytes} bytes" |
| `content` delta | Append to result panel character by character |
| `[DONE]` | Lock result panel, surface tokens + latency |

This is purely a frontend change and inherits the back-end Axis A.

### §3.4 Out of scope (deliberate, separate SDDs)

- Increasing Cloudflare timeout (operational, not architectural).
- New `mcp_loops.final_text` column — **rejected**, S3 ConversationRecord already holds `result`.
- Background-job polling pattern (`POST /jobs` → `GET /jobs/{id}`) — orthogonal API style, future SDD if needed.
- Token-by-token streaming during a single round's inference (already supported in the existing `stream_tokens` path; this SDD only changes the entry-handler's response framing).

---

## §4 Vision Alignment Check

| Vision pillar (`policies/architecture.md` §Vision) | Impact of this SDD |
|----------------------------------------------------|--------------------|
| Cluster-wide optimization | unchanged — dispatch / VRAM / thermal layers untouched |
| Multi-model co-residence | unchanged |
| 3-phase adaptive learning | unchanged |
| Self-healing / circuit breaker | strengthened — finalize_partial_on_cancel preserves observability traces on disconnect |
| Hexagonal architecture | preserved — Axis B uses existing `MessageStore` outbound port; Axis A uses existing inbound `chat_completions` handler |
| **S3 = SSOT for conversation** (`job-lifecycle.md`) | **enforced** — Tier B closes the cancel-path leak |
| Streaming as default for agentic loops | **established** — Tier A makes it the contract for MCP-routed traffic |

---

## §5 Tier A — MCP entry forces SSE streaming + heartbeat

> Goal: `mcp_ollama_chat` returns SSE response regardless of client's `stream` field. Final event carries the full bundled completion for legacy compatibility.
> Estimate: ~250 LoC. Branch: `feat/mcp-streaming-first`.

### §5.1 Files to modify

| File | Action |
| ---- | ------ |
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | `chat_completions`: when `should_intercept()` → call new `mcp_ollama_chat_stream`; deprecate `mcp_ollama_chat` non-stream path |
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | new `mcp_ollama_chat_stream(state, caller, req, conversation_id) -> impl IntoResponse` — returns SSE stream |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | `run_loop` already returns `RunLoopResult` per round; expose a `run_loop_streaming` that emits a `tokio::sync::mpsc::Receiver<RunLoopEvent>` per round + final |
| `crates/veronex/src/infrastructure/outbound/mcp/events.rs` (new) | `enum RunLoopEvent { ToolCall, ToolResult, ContentDelta, RoundComplete, Done }` |

### §5.2 SSE event taxonomy

| Event | Body shape |
|-------|------------|
| `:` (comment) | every 15 s — heartbeat / keep-alive |
| `event: mcp.tool_call` + `data: {round, tool, args}` | bridge initiated tool call |
| `event: mcp.tool_result` + `data: {round, tool, success, bytes}` | tool returned |
| (default `event: message`) `data: {choices:[{delta:{content:...}}]}` | model token delta — final round only |
| `data: {choices:[{finish_reason:...}]}` | terminal |
| `data: [DONE]` | OpenAI compat sentinel |

### §5.3 Acceptance criteria

- [ ] `curl --max-time 600 -N -H 'Cookie: ...' -H 'Accept: text/event-stream' POST /v1/chat/completions` with `stream:false` and an MCP-tool-using prompt returns 200 + `Content-Type: text/event-stream`
- [ ] Heartbeat comment lines appear at ≤ 30 s interval throughout the loop
- [ ] Cloudflare 524 not observed (run live on `veronex-api-dev.verobee.com` with 200K cold model)
- [ ] Final event contains a fully-formed `choices[0].message.content` (legacy aggregation works)
- [ ] Existing non-MCP path (`stream:false` no MCP bridge) **unchanged**

### §5.4 Tests

Unit:
- `mcp_ollama_chat_stream_emits_heartbeat_when_idle` — wiremock ollama returning slow response; assert `:` lines on the wire.
- `mcp_ollama_chat_stream_serialises_round_boundaries` — bridge mock emits 3 rounds; assert `mcp.tool_call`/`mcp.tool_result` event ordering.
- `mcp_ollama_chat_stream_emits_done_sentinel_at_end` — assert final `[DONE]` line.
- `non_mcp_chat_completions_unchanged` — regression — when `should_intercept()` returns false, response is single JSON body (existing assertion).

Integration: pre-existing `test/scripts/e2e/openai-compat.sh` extended with `--mcp-stream` flag.

---

## §6 Tier B — Cancel-resilient finalize_job (S3 always written)

> Goal: cancel path writes S3 ConversationRecord with whatever was accumulated. Preserves CDD invariant "S3 = SSOT for conversation".
> Estimate: ~120 LoC. Branch: `fix/finalize-on-cancel`. Independent of Tier A — can ship first.

### §6.1 Files to modify

| File | Action |
| ---- | ------ |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | Extract S3-write portion of `finalize_job` into reusable helper `persist_conversation_record(...) -> Option<()>`; call from cancel branch |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | Same helper called from "ownership lost" branch (currently line 245-247) |
| `crates/veronex/src/application/ports/outbound/message_store.rs` | (review) verify trait already supports the partial-record case; no change expected |
| `docs/llm/inference/job-lifecycle.md` | clarify S3 write is per-job-terminal (any path that ends the job — completion, cancel, ownership-lost) |

### §6.2 `persist_conversation_record` signature

```rust
async fn persist_conversation_record(
    message_store: &Option<Arc<dyn MessageStore>>,
    job: &InferenceJob,
    ts: &TokenStreamState,
    original_messages: &Option<serde_json::Value>,
    original_prompt: &str,
) {
    let Some(store) = message_store else { return };
    if ts.text.is_empty() && ts.tool_calls.is_empty() {
        // No useful state captured — skip the write.
        return;
    }
    let record = ConversationRecord {
        prompt: original_prompt.to_owned(),
        messages: original_messages.clone(),
        tool_calls: (!ts.tool_calls.is_empty())
            .then_some(serde_json::Value::Array(ts.tool_calls.clone())),
        result: (!ts.text.is_empty())
            .then_some(strip_think_blocks(ts.text.clone())),
    };
    let owner_id = job.account_id.or(job.api_key_id).unwrap_or(job.id.0);
    if let Err(e) = store.put_conversation(&owner_id, &job.id.0, &record).await {
        tracing::warn!(%job.id.0, "S3 conversation persist failed: {e}");
    }
}
```

`finalize_job` and `cancel_branch` both call this helper before their respective DB-side terminal updates. Idempotency: S3 PUT overwrites by key; if both paths fire (race), last write wins — same content.

### §6.3 Acceptance criteria

- [ ] Cancel mid-stream during MCP loop → S3 ConversationRecord exists with the partial tokens (verified via `mc cat veronex-conversations/{owner}/{date}/{job_id}.json.zst`)
- [ ] DB `failure_reason='lifecycle_failed'` (cancel-path) jobs surface their accumulated state in `/v1/dashboard/jobs/{id}` instead of "(no result stored)"
- [ ] Cancel BEFORE any token (queue cancel, lifecycle pre-stream-tokens cancel) → no S3 write (record would be useless)
- [ ] Existing happy-path `finalize_job` byte-identical (extracted helper, not rewritten logic)

### §6.4 Tests

- `cancel_after_first_tool_call_persists_to_s3` — mock provider emits one tool_call token, then test injects cancel; assert mock S3 received `put_conversation` with `tool_calls.is_some()`.
- `cancel_before_any_token_skips_s3` — assert mock S3 not called.
- `finalize_job_call_path_unchanged` — regression on existing happy-path.

### §6.5 Resume note

Independent of Tier A. Recommended to land first since it has zero cross-cutting impact and unblocks UI display of partial / cancelled conversations regardless of streaming-first work.

---

## §7 Tier C — Test panel UI: stream-first

> Branch: `feat/test-panel-stream` (web/). Blocked on Tier A. Frontend-only.

### §7.1 Files to modify

| File | Action |
| ---- | ------ |
| `web/app/api-test/components/...` | replace fetch-with-`stream:false` → SSE consumer (EventSource or fetch + getReader) |
| `web/app/api-test/components/...` | add tool-call progress timeline UI |
| `web/lib/api/chat-completion.ts` (or similar) | new `streamChatCompletion(req, handlers)` helper |

### §7.2 Acceptance criteria

- [ ] Test panel sends MCP-enabled request → progress steps (tool calls, tool results, content stream) render live
- [ ] Final result text displays as soon as last delta arrives
- [ ] User can cancel mid-loop via UI button → request aborts via SSE close → backend (Tier B) still persists what it has
- [ ] Token / latency / cost summary unchanged in final state

---

## §8 Post-implementation: CDD-sync (per `.add/doc-sync.md` + `cdd-feedback.md`)

| File | Update |
|------|--------|
| `docs/llm/inference/mcp.md` | New section "MCP response framing — server-driven SSE" documenting Axis A contract |
| `docs/llm/inference/job-lifecycle.md` | Clarify S3 write fires on ALL terminal paths (completion, cancel-with-partial-state, ownership-lost) |
| `docs/llm/inference/job-api.md` | `result_text` semantic clarified — None for tool-call-only OR pre-token cancellation; S3 record may exist with partial state |
| `docs/llm/flows/mcp.md` | Update flow diagram with SSE event timeline (round boundaries, heartbeats, final delta) |
| `docs/llm/policies/patterns/http.md` | New pattern entry "Server-driven SSE for variable-duration handlers" |
| `docs/llm/policies/architecture.md` | Add "MCP-routed `chat_completions` is response-framing-streaming-first regardless of client `stream` field" to Key Design Decisions |

### §8.1 Acceptance

- [ ] `grep -rn 'stream:false.*MCP\|MCP.*stream:false' docs/llm/` returns nothing implying that pairing is supported (it's no longer)
- [ ] Token-optimization compliance — no emoji, tables over prose, ≤H3
- [ ] All path references resolve (e.g. `flows/mcp.md` ASCII updated against actual handler code)

---

## §9 Post-implementation: Live verification (dev cluster)

Run on `veronex-api-dev.verobee.com` with `test-3` user, 200K-context model.

| # | Scenario | Expected |
|---|----------|----------|
| 1 | "금일 마이크론 주가에대해 알려줘" with `stream:false` | 200 + SSE response, no Cloudflare 524, final answer with web_search citation |
| 2 | Same prompt, abort after 30 s (Ctrl-C / UI cancel) | Backend `failure_reason=cancelled`; S3 record exists with partial tool_calls (if reached); UI shows "Tool Calls" panel with up-to-cancel state |
| 3 | Cold-load (200K) request with this fix | `lifecycle.ensure_ready` AlreadyLoaded fast-path or LoadCompleted (no regression on Tier C lifecycle work); SSE heartbeat keeps CDN alive throughout |
| 4 | Non-MCP raw chat (no MCP key) | Single JSON response when `stream:false`, SSE when `stream:true` (unchanged) |

Mark §0 row `[x] done` only after all four pass.

---

## §10 Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| Legacy clients depending on a specific `stream:false` JSON-body shape break | Final SSE event still carries full `chat.completion` JSON in the `data:` line — clients that join the SSE stream and read the last `data:` line before `[DONE]` get equivalent JSON |
| SSE proxy intermediaries strip empty comment lines (kill keep-alive) | Use `event: ping\ndata: 0\n\n` instead of `:` — explicit event still passes through stricter intermediaries |
| `EventSource` API does not support cookies cross-origin in some browsers | Use `fetch` + `ReadableStream` body parsing — already the pattern in veronex web (see `web/lib/sse.ts`) |
| S3 PUT failure on cancel path | Logged at `tracing::warn` (best-effort); DB row's `has_tool_calls`/tokens preserved — UI gracefully degrades |
| Tier B partial-write under hot cancel storm could pile up zstd compression | `MessageStore` adapter already does compression; bounded by per-job invocation count = 1 |

---

## §11 References

- CDD: `docs/llm/inference/mcp.md` (intercept rules, ACL), `docs/llm/inference/job-lifecycle.md` (S3 ConversationRecord schema), `docs/llm/inference/job-api.md` (UI fetch contract)
- Lifecycle SoD prior: `.specs/veronex/history/inference-lifecycle-sod.md`
- Cloudflare 524 reference: https://developers.cloudflare.com/support/troubleshooting/cloudflare-errors/troubleshooting-cloudflare-5xx-errors/#error-524
- OpenAI streaming reference: https://platform.openai.com/docs/api-reference/chat-streaming
- ollama#8006 (client-disconnect aborts load): https://github.com/ollama/ollama/issues/8006
- ADD workflow: `.add/feature-addition.md` step 5 → `.add/cdd-feedback.md`
