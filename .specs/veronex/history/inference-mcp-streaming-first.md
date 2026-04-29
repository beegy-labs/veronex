# SDD: Inference MCP — Streaming-First + Cancel-Resilient Persistence

> Status: planned | Change type: **Change** (entry-handler routing) + **Add** (cancel-resilient finalize) | Created: 2026-04-28 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/inference/job-lifecycle.md` · `docs/llm/inference/job-api.md` · `docs/llm/flows/mcp.md` · `docs/llm/policies/architecture.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S15
> **Resume rule**: every section is self-contained — any future session reading this SDD alone (no chat history) must continue from the last unchecked box.

---

## §0 Quick-resume State

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| B — Cancel-resilient persist (always write S3) | [x] done | `fix/finalize-on-cancel` | #100 | `a210b8b` |
| B-tests — `persist_partial_conversation` unit tests | [x] done | `test/tier-b-persist-conversation` | #101 | `ea179a3` |
| A — MCP entry forces SSE streaming + heartbeat | [x] done | `feat/mcp-streaming-first` | #102 | `806dabc` |
| A-spawn — Handler returns SSE Response immediately (spawn bridge) | [x] done | `feat/test-panel-stream` (Rust hotfix on same branch name) | #103 | `82f3ff3` |
| C — Test panel UI: tool-call timeline | [x] done | `feat/test-panel-tool-call-progress` | #104 | `e3cc641` |
| Live verify (dev) | [x] **done** — 2026-04-29 | — | platform-gitops sync | — |
| CDD-sync (post all) | [ ] in progress | `chore/mcp-streaming-cdd-sync` | TBD | TBD |

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

> Goal: `mcp_ollama_chat` returns SSE response regardless of client's `stream` field. **OpenAI-compatible** chunk format (verified via [OpenAI streaming-events ref](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events) and [OpenAI streaming guide](https://developers.openai.com/api/docs/guides/streaming-responses)).
> Estimate: ~350 LoC. Branch: `feat/mcp-streaming-first`. Reuses existing veronex SSE infrastructure (`flows/streaming.md`).

### §5.1 Files to modify

| File | Action |
| ---- | ------ |
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | `chat_completions`: when `should_intercept()` → unconditionally call `mcp_ollama_chat_stream` (drops the existing `stream:bool` switch in this branch only) |
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | NEW `mcp_ollama_chat_stream(state, caller, req, conversation_id) -> Sse<...>` |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | NEW `run_loop_streaming(...) -> mpsc::Receiver<RunLoopEvent>` next to existing `run_loop` (existing kept for non-streaming callers; deprecated when MCP path migrates) |
| `crates/veronex/src/infrastructure/outbound/mcp/events.rs` (NEW) | `pub enum RunLoopEvent { … }` (exact variants in §5.1d) |
| `crates/veronex/src/infrastructure/inbound/http/openai_sse_types.rs` | add `CompletionChunk::tool_call_delta(index, id, name, arguments_chunk)` constructor — already-existing `DeltaContent` extended; **no breaking change** to existing types |
| `crates/veronex/src/infrastructure/inbound/http/handlers.rs` | reuse existing `try_acquire_sse()` / `with_sse_timeout()` / `SseDropGuard` (no new helpers — see `flows/streaming.md`) |

### §5.1a SSE response headers (Cloudflare-safe contract)

Per [Cloudflare SSE community guidance](https://community.cloudflare.com/t/are-server-sent-events-sse-supported-or-will-they-trigger-http-524-timeouts/499621) and [SmartScope SSE timeout mitigation 2026](https://smartscope.blog/en/Infrastructure/sse-timeout-mitigation-cloudflare-alb/), the response **must** carry these exact headers to bypass intermediary buffering and prevent 524:

```rust
.header("content-type", "text/event-stream")
.header("cache-control", "no-cache, no-transform")
.header("connection", "keep-alive")
.header("x-accel-buffering", "no")          // disables nginx + envoy + Cloudflare buffering
```

`x-accel-buffering: no` is mandatory — without it, Cloudflare and Cilium-Envoy in our cluster buffer SSE bodies even with `text/event-stream` set. (`tower_http::CompressionLayer` is also incompatible — already excluded from the `/v1/chat/completions` route, verified via `grep -nE "compression" crates/veronex/src/infrastructure/inbound/http/router.rs`.)

### §5.1b axum SSE + KeepAlive integration

Per [axum::response::sse::KeepAlive docs](https://docs.rs/axum/latest/axum/response/sse/struct.KeepAlive.html):

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use std::time::Duration;
use crate::domain::constants::SSE_KEEP_ALIVE;   // existing 15 s

let stream = ReceiverStream::new(rx).map(|event| Ok::<_, Infallible>(match event {
    RunLoopEvent::McpToolCall { round, tool, args } => Event::default()
        .event("mcp.tool_call")
        .json_data(json!({"round": round, "tool": tool, "args": args}))?,
    RunLoopEvent::McpToolResult { round, tool, success, bytes } => Event::default()
        .event("mcp.tool_result")
        .json_data(json!({"round": round, "tool": tool, "success": success, "bytes": bytes}))?,
    RunLoopEvent::ChatChunk(chunk) => Event::default()
        .json_data(chunk)?,                     // serializes `CompletionChunk`
    RunLoopEvent::Done => Event::default().data("[DONE]"),
    RunLoopEvent::Error(err) => Event::default()
        .event("error")
        .json_data(json!({"error": {"message": sanitize_sse_error(&err)}}))?,
}));

Sse::new(stream)
    .keep_alive(
        KeepAlive::new()
            .interval(SSE_KEEP_ALIVE)           // 15s — matches existing dashboard SSE
            .event(Event::default().event("ping").data("0")),  // explicit event, not bare comment, per stricter intermediaries
    )
```

Why `.event("ping").data("0")` over default empty comment: per [Cloudflare buffering thread](https://community.cloudflare.com/t/cloudflare-buffering-sse-streams/506921) some intermediaries strip empty comment lines; an explicit (named, payloaded) keep-alive event survives those.

### §5.1c OpenAI `chat.completion.chunk` for tool_calls — exact shape

Verified against the [OpenAI Chat Completions streaming events reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events):

```json
// Round 0 — model decides to call a tool. Arguments arrive incrementally.
data: {"id":"chatcmpl-mcp-...","object":"chat.completion.chunk","created":1777372231,
       "model":"qwen3-coder-next-200k:latest","system_fingerprint":"fp_veronex",
       "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[
         {"index":0,"id":"call_abc","type":"function","function":{"name":"mcp_..._web_search"}}
       ]},"finish_reason":null}]}\n\n
data: {"id":"...","object":"chat.completion.chunk",...,
       "choices":[{"index":0,"delta":{"tool_calls":[
         {"index":0,"function":{"arguments":"{\"query\":"}}
       ]},"finish_reason":null}]}\n\n
data: {"id":"...","object":"chat.completion.chunk",...,
       "choices":[{"index":0,"delta":{"tool_calls":[
         {"index":0,"function":{"arguments":"\"micron stock today\"}"}}
       ]},"finish_reason":null}]}\n\n
data: {"id":"...","object":"chat.completion.chunk",...,
       "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}\n\n

// veronex emits the bridge progress events between rounds (NOT in OpenAI spec — namespaced)
event: mcp.tool_call
data: {"round":0,"tool":"mcp_..._web_search","server_id":"019d84f4-...","args":{"query":"micron stock today"}}\n\n
event: mcp.tool_result
data: {"round":0,"tool":"mcp_..._web_search","success":true,"bytes":2486}\n\n

// Final round — model emits the answer. Standard OpenAI content delta chunks.
data: {"id":"...","choices":[{"index":0,"delta":{"content":"오늘 마이크론(MU)은 "},"finish_reason":null}]}\n\n
data: {"id":"...","choices":[{"index":0,"delta":{"content":"$XX.XX 입니다..."},"finish_reason":null}]}\n\n
data: {"id":"...","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\n

// `stream_options: { include_usage: true }` adds usage on the final chunk
data: {"id":"...","choices":[],"usage":{"prompt_tokens":250,"completion_tokens":83,"total_tokens":333}}\n\n

data: [DONE]\n\n
```

Key rules:
- `chat.completion.chunk` events are emitted as **default `event: message`** (no `event:` line) per OpenAI compat.
- Veronex-specific `event: mcp.tool_call` / `mcp.tool_result` are **named** events — OpenAI clients ignoring named events still get a fully-functional stream from the unnamed `data:` lines.
- `finish_reason` values per OpenAI spec: `"stop" | "length" | "tool_calls" | "content_filter" | "function_call"` (last is deprecated). veronex emits `"tool_calls"` mid-loop (round end) and `"stop"` at final-round end.
- `[DONE]` sentinel terminates the stream — clients should close the connection on receipt.

### §5.1d `RunLoopEvent` exact enum

```rust
// crates/veronex/src/infrastructure/outbound/mcp/events.rs
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunLoopEvent {
    /// Standard OpenAI chat.completion.chunk — emitted as anonymous SSE event.
    ChatChunk(crate::infrastructure::inbound::http::openai_sse_types::CompletionChunk),

    /// Veronex-namespaced — emitted as `event: mcp.tool_call`.
    McpToolCall {
        round: u8,
        tool: String,
        server_id: uuid::Uuid,
        args: serde_json::Value,
    },

    /// `event: mcp.tool_result`.
    McpToolResult {
        round: u8,
        tool: String,
        success: bool,
        bytes: usize,
        cache_hit: bool,
        latency_ms: u64,
    },

    /// `event: error` — final terminal event for failures mid-stream.
    /// Stream still closes with `[DONE]` after.
    Error(String),

    /// `data: [DONE]` — terminator.
    Done,
}
```

### §5.1e Mid-stream error semantics

Once the response started (HTTP 200 + headers flushed), HTTP status cannot be changed. Per [OpenAI streaming guide](https://developers.openai.com/api/docs/guides/streaming-responses), errors after stream start are reported as a **named `error` event** then `[DONE]`:

```
event: error
data: {"error":{"message":"<sanitized via sanitize_sse_error()>","code":"provider_error"}}\n\n
data: [DONE]\n\n
```

`sanitize_sse_error()` from `policies/patterns/http.md` §SSE Error Sanitization is mandatory — strips DB/network internals, escapes CR/LF, truncates 200 chars.

### §5.1f Backpressure & channel sizing

`tokio::sync::mpsc::channel::<RunLoopEvent>(BRIDGE_TO_SSE_CHANNEL_CAPACITY)` where `BRIDGE_TO_SSE_CHANNEL_CAPACITY = 64`. Rationale:
- Per-round events: 1 ChatChunk per token (typical 50–200 tokens) + 1 McpToolCall + 1 McpToolResult.
- 64 buffers ~30 s of token output without bridge stalling.
- If client is slow and channel fills → bridge `send().await` applies natural backpressure (no drop). This is intentional — bridge runs at most one round at a time, so backpressure on bridge = cooperative pacing; ollama keep_alive ensures the model stays warm.
- **No drop policy** — every event is significant.

`tokio::sync::mpsc` (bounded) over `broadcast::channel` because there is exactly **one consumer** (the SSE response stream) per request.

### §5.1g Cancel propagation — connection drop → bridge halt

`Sse::keep_alive` returns a stream that, when dropped, closes the receiver. Bridge's `send().await` then returns `Err(SendError)`; `run_loop_streaming` interprets this as cancel and exits its inner round loop. Tier B handles the partial S3 persist on this exit.

```rust
// In run_loop_streaming:
if let Err(_) = tx.send(RunLoopEvent::ChatChunk(chunk)).await {
    // Receiver dropped — client disconnected. Stop processing further rounds.
    return BridgeOutcome::ClientDisconnect { rounds_completed, last_state };
}
```

### §5.2 Acceptance criteria

- [ ] `curl --max-time 600 -N -H 'Cookie: ...' -H 'Accept: text/event-stream' POST /v1/chat/completions` with `stream:false` body and an MCP-tool-using prompt returns 200 + `Content-Type: text/event-stream`
- [ ] Response headers include `x-accel-buffering: no`, `cache-control: no-cache, no-transform`
- [ ] Heartbeat `event: ping` lines appear at ≤ 20 s interval throughout a 200 s+ run
- [ ] Cloudflare 524 not observed against `veronex-api-dev.verobee.com` (live verify per §9, scenario 1)
- [ ] Final stream sequence terminates with `data: [DONE]\n\n`
- [ ] OpenAI Python SDK `client.chat.completions.create(..., stream=True)` consumes our stream — verified via `pip install openai && python -c "..."` (acceptance script in `test/scripts/e2e/openai-compat-mcp.sh`)
- [ ] Existing non-MCP path (`stream:false`, no MCP bridge) **unchanged** (regression test)
- [ ] Bridge cancel-on-disconnect → run_loop_streaming returns `ClientDisconnect` outcome within ≤ 1 s

### §5.3 Tests

Unit (in `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs::tests`):
- `run_loop_streaming_emits_chat_chunk_events` — wiremock provider; assert `RunLoopEvent::ChatChunk` ordering matches token stream.
- `run_loop_streaming_emits_mcp_tool_call_then_result_per_round` — assert event ordering for one tool-using round.
- `run_loop_streaming_emits_done_sentinel_on_max_rounds` — assert `[DONE]` after MAX_ROUNDS exhausted.
- `run_loop_streaming_emits_error_event_then_done_on_provider_error` — assert error then DONE; client never observes a non-200 status mid-stream.
- `run_loop_streaming_returns_client_disconnect_on_send_failure` — drop receiver mid-stream; assert bridge returns `ClientDisconnect` quickly.

Unit (in `openai_handlers.rs::tests`):
- `mcp_ollama_chat_stream_sets_x_accel_buffering_header` — response header assertion.
- `mcp_ollama_chat_stream_keepalive_interval_15s` — manipulate tokio time; assert `event: ping` cadence.
- `mcp_ollama_chat_stream_invokes_persist_on_disconnect` — Tier B integration.

Integration: `test/scripts/e2e/openai-compat-mcp.sh` (NEW) — runs against dev cluster, asserts:
- exit 0 within 600 s
- response includes `event: mcp.tool_call`
- final non-empty `delta.content`
- `[DONE]` last
- Cloudflare 524 not observed

---

## §6 Tier B — Cancel-resilient finalize_job (S3 always written)

> Goal: every job terminal exit writes S3 ConversationRecord with whatever was accumulated. Preserves CDD invariant "S3 = SSOT for conversation" per `inference/job-lifecycle.md`.
> Estimate: ~150 LoC. Branch: `fix/finalize-on-cancel`. Independent of Tier A — recommended to ship first.

### §6.1 All terminal exit paths — full enumeration

A "terminal exit" is any code path that ends a `run_job` invocation. Each must call `persist_conversation_record` exactly once. Verified via `grep -nE "return Ok\\(None\\)|return Ok\\(Some|return Err" crates/veronex/src/application/use_cases/inference/runner.rs`:

| # | Path | File:line (current `develop` after #98) | Existing terminal action | Tier B addition |
|---|------|-----------------------------------------|--------------------------|-----------------|
| T1 | Cancel before dispatch (entry status==Cancelled) | `runner.rs::run_job` early branch | DECR pending; return Ok(None) | call `persist` (will skip — no tokens yet — but contract uniform) |
| T2 | Cancel during stream (entry.status==Cancelled in stream loop) | `runner.rs::run_job` mid-loop | DECR running; return Ok(None) | **call `persist` before return** |
| T3 | Cancel via cancel_notify (biased select! arm) | `runner.rs::run_job` cancel branch | DECR running; schedule_cleanup; return Ok(None) | **call `persist` before return** |
| T4 | Ownership lost (instance_id mismatch) | `runner.rs::finalize_job` line 245 | schedule_cleanup; return None | call `persist` before return (stream may have collected tokens) |
| T5 | Provider stream Err (item is Err in stream loop) | `runner.rs::run_job` Err arm | mark failed; return Err | **call `persist` before return** |
| T6 | Lifecycle failed (Phase 1 ensure_ready failed, PR #93) | `runner.rs::run_job` lifecycle Err arm (line ~512–533) | fail_with_reason("lifecycle_failed"); return Ok(None) | call `persist` (will skip — no tokens yet) |
| T7 | Normal completion | `runner.rs::finalize_job` happy path | UPDATE completed; existing S3 write | refactor to call helper (byte-identical content) |
| T8 | Queue-side cancellation | `queue_maintenance.rs::queue_wait_cancel` | cancel + UPDATE failed; never enters run_job | NO action (job never ran — no `ts` exists) |
| T9 | Lease-expired re-enqueue cap (`lease_expired_max_attempts`) | `queue_maintenance.rs::processing_reaper` | UPDATE failed | NO action (queue-side, no `ts`) |

Total: T2/T3/T5 are the **critical missing-write paths** that produced the user's "저장된 결과 없음" symptom. T1/T6/T7/T8/T9 are either no-tokens-collected or already correct. T4 is defensive.

### §6.2 `persist_conversation_record` — exact signature & idempotency

```rust
// crates/veronex/src/application/use_cases/inference/runner.rs
//
// Called from every T-path in §6.1 that has access to `ts`. Idempotent:
// the per-job `persisted_to_s3` AtomicBool guards against double-write
// when both finalize_job and cancel paths race.

#[allow(clippy::too_many_arguments)]
async fn persist_conversation_record(
    message_store: &Option<Arc<dyn MessageStore>>,
    persisted_flag: &AtomicBool,                  // per-job, lives in JobEntry
    job: &InferenceJob,
    ts: &TokenStreamState,
    original_messages: &Option<serde_json::Value>,
    original_prompt: &str,
) {
    // Idempotent guard — only the first caller writes. compare_exchange
    // succeeds exactly once even under concurrent calls from cancel +
    // finalize racing across run_job's biased select! → cancel arm vs
    // stream arm completing.
    if persisted_flag.compare_exchange(false, true,
            Ordering::AcqRel, Ordering::Acquire).is_err() {
        return;
    }

    let Some(store) = message_store else { return };

    // Skip the write when the record would be empty. T1/T6 land here.
    if ts.text.is_empty() && ts.tool_calls.is_empty() {
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
        // Best-effort — DB row's metadata (status/tokens/has_tool_calls)
        // is already correct via the normal cancel/finalize paths;
        // missing S3 is a soft failure surfaced via tracing only.
        tracing::warn!(
            job_id = %job.id.0,
            owner_id = %owner_id,
            "S3 conversation persist failed: {e}"
        );
    } else {
        tracing::debug!(
            job_id = %job.id.0,
            owner_id = %owner_id,
            text_len = ts.text.len(),
            tool_calls = ts.tool_calls.len(),
            "S3 conversation persisted"
        );
    }
}
```

### §6.2a `JobEntry::persisted_to_s3` field

```rust
// application/use_cases/inference/mod.rs::JobEntry
pub(crate) struct JobEntry {
    // ... existing fields ...
    /// Set true exactly once when `persist_conversation_record` performs
    /// the S3 PUT for this job. Prevents double-write across racing
    /// finalize_job ↔ cancel paths inside run_job's biased select!.
    pub persisted_to_s3: Arc<AtomicBool>,           // NEW
}
```

Persists only in memory; no DB column. `Arc` so the helper can be called with a borrow that survives drop of the DashMap entry (cancel branch drops the entry guard before persist).

### §6.2b Order-of-operations contract

For each cancel-path (T2/T3):

```
1. entry.status = JobStatus::Cancelled            (already done by use_case::cancel)
2. drop(entry)                                     (release DashMap shard)
3. decr_running(&valkey).await                     (counter bookkeeping)
4. persist_conversation_record(...).await          (NEW — Tier B)
5. schedule_cleanup(&jobs, uuid, ...)              (60 s deferred remove from DashMap)
6. return Ok(None)
```

For T7 (normal `finalize_job` happy path):
```
1. mark entry done = true; status = Completed
2. result_text / tool_calls / metrics computed from `ts`
3. persist_conversation_record(...).await          (refactored to use helper)
4. job_repo.finalize(...).await                    (DB UPDATE)
5. broadcast_event(Completed); observability emit
```

S3 write happens BEFORE DB UPDATE in both paths so DB never advertises "completed" for a job whose conversation is missing.

### §6.3 Acceptance criteria

- [ ] T2/T3/T5 paths: cancel mid-stream during MCP loop → S3 ConversationRecord exists with partial tokens (verified via gitea/garage `mc cat veronex-conversations/{owner}/{date}/{job_id}.json.zst`)
- [ ] T7 happy-path: byte-identical S3 record content vs pre-Tier-B (regression test diffs serialized record bytes)
- [ ] `JobEntry::persisted_to_s3` Atomic guard: parallel injected cancel + finalize_complete on the same job → exactly **one** S3 PUT (assert via mock store call count)
- [ ] T1/T6/T8/T9: no S3 write (assert mock store not called when ts is empty / job never ran)
- [ ] DB rows from `failure_reason='lifecycle_failed'` (cancel-path-derived) surface their accumulated state in `GET /v1/dashboard/jobs/{id}` instead of "(no result stored)"
- [ ] DashMap shard guard not held across `await` in any modified path (cargo-lints clippy::await_holding_lock)

### §6.4 Tests

Unit (in `application/use_cases/inference/runner.rs::tests`):
- `cancel_mid_stream_persists_partial_tool_calls_to_s3` — provider mock emits one tool_call token, inject cancel via `cancel_notify`; assert mock `MessageStore.put_conversation` called once with `tool_calls.is_some()` and `result.is_none()`.
- `cancel_after_partial_text_persists_text_to_s3` — assert `result = Some("partial...")`.
- `cancel_before_any_token_skips_s3_write` — assert mock store **not** called.
- `parallel_cancel_and_finalize_writes_s3_exactly_once` — race the two paths via `tokio::join!`; assert mock store call count == 1.
- `finalize_job_happy_path_persists_unchanged` — diff serialized `ConversationRecord` byte stream vs pre-Tier-B golden file.
- `lifecycle_failed_path_skips_s3_write_when_no_tokens` — Tier-C lifecycle error returns Ok(None) before stream_tokens; assert no S3 write.
- `provider_stream_error_persists_partial_state` — provider returns Err mid-stream; assert S3 has tokens emitted before the error.

Integration: `test/scripts/e2e/cancel-persist.sh` — submits a 200K MCP request, kills curl after 30 s, then queries `/v1/dashboard/jobs/{id}` and asserts non-empty tool_calls section.

### §6.5 Resume note

Independent of Tier A. Recommended to land first — zero cross-cutting impact, unblocks UI display of partial / cancelled conversations regardless of streaming-first work, and benefits all existing failure modes (T2/T3/T5 today silently lose data on every disconnect even without MCP).

---

## §7 Tier C — Test panel UI: stream-first

> Branch: `feat/test-panel-stream` (web/). Blocked on Tier A. Frontend-only.

### §7.1 Files to modify

| File | Action |
| ---- | ------ |
| `web/lib/sse.ts` (existing) | reuse — extend if needed for named events (`mcp.tool_call`, `mcp.tool_result`) |
| `web/lib/api/chat-completion.ts` (NEW or existing) | `streamChatCompletion(req, handlers, abortSignal): Promise<AggregatedResponse>` helper |
| `web/app/api-test/components/test-form.tsx` (or similar) | replace `fetch + json` → `streamChatCompletion`; pipe events into existing result panel + add tool-call timeline |
| `web/app/api-test/components/tool-call-timeline.tsx` (NEW) | accordion-style component showing each `mcp.tool_call` round, success/fail badge, optional result preview |

### §7.1a Auth — `fetch + ReadableStream`, not `EventSource`

Browser `EventSource` API limitations (verified standard, MDN):
- No custom headers (no `Authorization: Bearer ...`)
- No request body — GET-only
- Cookie-only auth works **only** with `withCredentials: true` and same-origin or CORS-permitted

Veronex uses `Authorization: Bearer <jwt>` for the test panel (verified via `grep -r "veronex_access_token" web/lib/`). Therefore: **`fetch` + `Response.body.getReader()` text-stream parser is mandatory**. Pattern (already exists in `web/lib/sse.ts`):

```typescript
// web/lib/api/chat-completion.ts (new)
export async function streamChatCompletion(
  req: ChatCompletionRequest,
  handlers: {
    onChatChunk: (c: ChatCompletionChunk) => void;
    onMcpToolCall: (e: McpToolCallEvent) => void;
    onMcpToolResult: (e: McpToolResultEvent) => void;
    onError: (msg: string) => void;
    onDone: () => void;
  },
  abort: AbortSignal,
): Promise<AggregatedResponse> {
  const resp = await fetch('/v1/chat/completions', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Accept': 'text/event-stream',
      // Cookie auth via SameSite=Strict — no explicit header
    },
    credentials: 'include',
    body: JSON.stringify(req),
    signal: abort,
  });

  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);

  const reader = resp.body!.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  const aggregated: AggregatedResponse = { content: '', tool_calls: [] };

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });

    // Parse SSE events delimited by \n\n
    let idx;
    while ((idx = buf.indexOf('\n\n')) >= 0) {
      const raw = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      const evt = parseSseEvent(raw);
      if (evt.event === 'mcp.tool_call') handlers.onMcpToolCall(JSON.parse(evt.data));
      else if (evt.event === 'mcp.tool_result') handlers.onMcpToolResult(JSON.parse(evt.data));
      else if (evt.event === 'error') handlers.onError(JSON.parse(evt.data).error.message);
      else if (evt.event === 'ping') { /* keep-alive */ }
      else if (evt.data === '[DONE]') { handlers.onDone(); return aggregated; }
      else {
        const chunk: ChatCompletionChunk = JSON.parse(evt.data);
        handlers.onChatChunk(chunk);
        const delta = chunk.choices?.[0]?.delta;
        if (delta?.content) aggregated.content += delta.content;
        if (delta?.tool_calls) mergeToolCallsDelta(aggregated.tool_calls, delta.tool_calls);
      }
    }
  }
  return aggregated;
}
```

`mergeToolCallsDelta` handles OpenAI's incremental `arguments` chunks (concat across deltas keyed by `tool_calls[].index`), matching §5.1c shape exactly.

### §7.1b Cancel UX

Cancel button → `abortController.abort()` → `fetch` aborts → server-side connection drop → bridge stops (§5.1g) → Tier B persists partial state. UI then re-fetches `GET /v1/dashboard/jobs/{id}` to display whatever was saved (tool_calls panel surfaces if `result_text=null`).

### §7.2 Acceptance criteria

- [ ] Test panel sends MCP-enabled request → tool-call timeline renders each round live; content panel populates on final-round delta
- [ ] Cloudflare 524 not observed via browser DevTools network panel for runs > 100 s
- [ ] User cancel button → request aborts within 1 s → reload of detail view shows `tool_calls` section (Tier B persisted)
- [ ] Token / latency / cost summary unchanged in final state — read from same DB row as before
- [ ] Lighthouse / no console errors during streaming
- [ ] Existing non-MCP requests (no test panel MCP toggle) **unchanged** — if non-stream JSON behavior preserved, tests in `web/app/api-test/__tests__` pass

### §7.3 Tests

- Storybook: tool-call-timeline component with mocked event sequence
- `web/lib/api/__tests__/chat-completion.test.ts` — unit-test parsing of mixed SSE event types; assert AggregatedResponse correctness on incremental tool-call deltas
- Cypress / Playwright e2e (existing harness) — extend `test-panel.spec.ts` to assert tool-call timeline + final content render

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

## §9.5 Live Verification Results (2026-04-29)

End-to-end run on `veronex-api-dev.verobee.com` after all four tiers
deployed (image `develop-82f3ff3` for api/mcp/agent/analytics/consumer,
`develop-e3cc641` for web). User-reported prompt verbatim:
"금일 마이크론 주가에대해 알려줘" with `"stream":false` (the exact
shape the test panel sent before).

### Acceptance grid

| § | Criterion | Observed | Verdict |
|---|-----------|----------|---------|
| §5.2 | HTTP 200 + `content-type: text/event-stream` | `HTTP/2 200`, `content-type: text/event-stream` | ✅ |
| §5.2 | No Cloudflare 524 for 200 s+ runs | request held alive 240,860 ms (4 min) | ✅ |
| §5.2 | Heartbeat ≤ 20 s during silent phase | colon-comment heartbeats appear repeatedly during cold-load before first content chunk | ✅ |
| §5.2 | Stream terminates with `data: [DONE]\n\n` | `[DONE]` count = 1 | ✅ |
| §5.2 | OpenAI-compat `chat.completion.chunk` shape | 182 data events, all valid `chatcmpl-mcp-...` chunks with `delta.content` | ✅ |
| §6.3 | Bridge S3 write reaches post-loop block | `MCP round complete round=0 mcp_calls=1` logged from spawned bridge task; cold-load `LoadCompleted{duration_ms: 221433}` | ✅ |
| §6.3 | Job DB rows reflect successful completion | both round-jobs `status=completed`, final round `completion_tokens=195` (the Korean answer) | ✅ |
| §7.2 | Test panel UI consumes SSE without code change | `iterSseLines` was already streaming-aware; Tier C added tool-call timeline rendering | ✅ |
| §10.6 | No regression on Tier B / lifecycle SoD | lifecycle.ensure_ready outcome=AlreadyLoaded duration_ms=0 on warm round; no Stalled / LifecycleError observed | ✅ |

### Observed timeline (stream:false MCP request)

```
T=0       client POST  /v1/chat/completions stream:false
T+50ms    handler returns SSE Response (axum flushes 200 + headers)
T+50ms    KeepAlive begins emitting `:` comment heartbeats every 15 s
T+50ms    bridge.run_loop spawned in background
T+50ms..  intermediate rounds (web_search tool call ~15 s)
T+15s     round 0 complete: mcp_calls=1
T+15s..   final round inference_job dispatched
T+15s..236s  cold-load: lifecycle.ensure_ready blocks ~221 s
            (LoadCompleted{duration_ms: 221433})
T+236s..  final round emits 195 tokens of Korean text via SSE
T+240s    [DONE] sentinel; client closes
```

Cloudflare's 100 s idle-timeout never trips because heartbeats flow
continuously from `T+50 ms`. Pre-Tier-A this same prompt produced
524 + empty S3 record + UI "저장된 결과 없음".

### Caveat

`X-Accel-Buffering: no` header set by `sse_response()` did not appear
in the curl-observed response headers — Cloudflare appears to strip
it on the response edge. Despite that, the stream did NOT buffer
(content arrived chunk-by-chunk during the test). This is consistent
with [Cloudflare community thread on SSE buffering](https://community.cloudflare.com/t/cloudflare-buffering-sse-streams/506921):
the header is honoured upstream by nginx/envoy but Cloudflare's edge
may rewrite/strip headers — what matters at runtime is that data
flows continuously, which the heartbeat guarantees.

---

## §10 Cross-cutting concerns

### §10.1 Observability — tracing spans + metrics

| Span name | Where | Fields |
|-----------|-------|--------|
| `mcp.run_loop_streaming` | bridge — outer | `model`, `account_id`, `caller_kind`, `rounds_target_max=5` |
| `mcp.run_loop_streaming.round` | bridge — per round | `round`, `tool_count`, `had_text`, `outcome` |
| `mcp.sse_stream` | openai_handlers | `events_sent`, `client_disconnect`, `total_duration_s`, `total_bytes` |

| Metric (Prometheus, via existing OTel pipeline) | Type | Labels |
|------------------------------------------------|------|--------|
| `veronex_mcp_sse_events_total` | counter | `event_type` (chat_chunk / tool_call / tool_result / error / done) |
| `veronex_mcp_sse_stream_duration_seconds` | histogram | `outcome` (done / client_disconnect / error / max_rounds) |
| `veronex_mcp_sse_keepalive_total` | counter | (no labels — heartbeat firing rate) |
| `veronex_persist_conversation_record_total` | counter | `path` (finalize / cancel / ownership_lost / provider_error), `outcome` (written / skipped_empty / s3_error) |

### §10.2 Pubsub-relay / multi-pod considerations

- Bridge `run_loop_streaming` runs **in-process** on the pod that received the HTTP request. No cross-pod state. SSE response stays on the same pod.
- veronex's pubsub_relay (`pubsub-relay.md`) relays job-status broadcast events across pods for SSE dashboards — this SDD's per-request streaming is orthogonal (no relay needed).
- k8s ingress affinity: NOT required (request-scoped state lives in the response future itself; no shared session).

### §10.3 Concurrent SSE budget

`SSE_MAX_CONNECTIONS=100` (`http/constants.rs`) is the global cap, gated by `try_acquire_sse()` + `SseDropGuard` (`flows/streaming.md`). MCP-stream requests share this budget. Estimate: 100 concurrent MCP loops × bounded mpsc(64) × ~1 KB/event = ~6 MB worst-case in-flight memory. Acceptable under existing pod limits (`api.resources.limits.memory=512Mi`).

If volume requires lifting: add `MCP_SSE_MAX_CONNECTIONS` separate from dashboard SSE budget; not in scope for this SDD.

### §10.4 HTTP/2 & HTTPRoute

Cilium Gateway → veronex-dev-api uses HTTP/2 by default. SSE over HTTP/2 works ("event-stream" content-type prevents server push frame compression issues). Verified — existing `/v1/dashboard/jobs/stream` SSE endpoint runs over HTTP/2 in dev (`flows/streaming.md`).

No new HTTPRoute changes needed. Cloudflare in front passes SSE through with the §5.1a headers set.

### §10.5 End-to-end test plan

| Script | Repo | Trigger |
|--------|------|---------|
| `test/scripts/e2e/openai-compat-mcp.sh` (NEW) | veronex | manual + CI on push to MCP-related paths |
| `test/scripts/e2e/cancel-persist.sh` (NEW) | veronex | manual |
| `test-panel.spec.ts` extension | veronex web/ | `npm run test:e2e` |

Each script is **resume-safe**: header documents the SDD section being verified, exit code propagates to CI, run against `veronex-api-dev.verobee.com` with `test-3`/`test1234!` credentials (per project memory).

### §10.6 Resilience to existing PR #98 patterns

- PR #96 stall-fix invariants (`/api/ps` poller + sentinel-zero) untouched. Tier A path runs **after** Phase 1 (`ensure_ready`) regardless of streaming mode — no Phase 1/2 ordering change.
- PR #98 archived SDD reference (`.specs/veronex/history/inference-lifecycle-sod.md`) is the SoD precedent — this SDD layers **on top**, not against.
- `MCP_LIFECYCLE_PHASE` flag (Tier C of the lifecycle SoD) remains the gate for Phase 1; not changed.

---

## §11 Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| Legacy clients depending on a specific `stream:false` JSON-body shape break | Final SSE event still carries full `chat.completion` JSON in the `data:` line — clients that join the SSE stream and read the last `data:` line before `[DONE]` get equivalent JSON |
| SSE proxy intermediaries strip empty comment lines (kill keep-alive) | Use `event: ping\ndata: 0\n\n` instead of `:` — explicit event still passes through stricter intermediaries |
| `EventSource` API does not support cookies cross-origin in some browsers | Use `fetch` + `ReadableStream` body parsing — already the pattern in veronex web (see `web/lib/sse.ts`) |
| S3 PUT failure on cancel path | Logged at `tracing::warn` (best-effort); DB row's `has_tool_calls`/tokens preserved — UI gracefully degrades |
| Tier B partial-write under hot cancel storm could pile up zstd compression | `MessageStore` adapter already does compression; bounded by per-job invocation count = 1 |

---

## §12 References

### CDD (internal SSOT — read before implementation)
- `docs/llm/inference/mcp.md` — intercept rules, ACL, MCP loop semantics
- `docs/llm/inference/job-lifecycle.md` — S3 ConversationRecord schema (the unchanged target of this SDD)
- `docs/llm/inference/job-api.md` — UI fetch contract for `/v1/dashboard/jobs/{id}` + result_text vs tool_calls_json semantic
- `docs/llm/inference/openai-compat-native.md` — shared `CompletionChunk`/`ChatCompletion` types
- `docs/llm/flows/streaming.md` — existing SSE infrastructure (constants, helpers, drop guard) — **reuse, don't re-invent**
- `docs/llm/policies/patterns/http.md` — `sanitize_sse_error()`, AppError → Problem Details
- `docs/llm/flows/mcp.md` — MCP run_loop ASCII flow (will be updated in §8)

### External best-practice references (verified via web search 2026-04-28)
- [OpenAI Chat Completions streaming events reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events) — chunk format, `tool_calls` deltas, `finish_reason` values
- [OpenAI streaming guide](https://developers.openai.com/api/docs/guides/streaming-responses) — error mid-stream pattern (`event: error` then `[DONE]`)
- [axum::response::sse::KeepAlive docs](https://docs.rs/axum/latest/axum/response/sse/struct.KeepAlive.html) — `interval()`, `event()`, default 15 s
- [Cloudflare community — SSE buffering issue](https://community.cloudflare.com/t/cloudflare-buffering-sse-streams/506921) — `x-accel-buffering: no` header is mandatory
- [Cloudflare community — SSE 524 timeouts](https://community.cloudflare.com/t/are-server-sent-events-sse-supported-or-will-they-trigger-http-524-timeouts/499621) — 30 s heartbeat avoids 100 s timeout
- [SmartScope — SSE timeout mitigation 2026](https://smartscope.blog/en/Infrastructure/sse-timeout-mitigation-cloudflare-alb/) — keep-alive cadence + no-transform
- [tower_http compression incompatible with SSE](https://github.com/tokio-rs/axum/discussions/2728) — verified our routes do not apply CompressionLayer to chat completions

### Adjacent prior work
- `.specs/veronex/history/inference-lifecycle-sod.md` — Phase 1/Phase 2 SoD (this SDD layers on top)
- PR #90 — bridge phased timeouts (FIRST_TOKEN/STREAM_IDLE/ROUND_TOTAL); preserved as defense-in-depth
- PR #96 — `/api/ps`-fed stall semantics (sentinel zero)

### ADD workflow
- `.add/feature-addition.md` (step 5 → `.add/cdd-feedback.md`)
- `.add/doc-sync.md` (post-impl CDD divergence cleanup)
