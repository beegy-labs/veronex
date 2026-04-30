# SDD: MCP tool_calls audit exposure + loop convergence forcing

> Status: planned | Change type: **Fix** (data exposure gap + missing loop termination invariant) | Created: 2026-04-30 | Owner: TBD
> CDD basis: `docs/llm/inference/mcp.md` · `docs/llm/inference/mcp-schema.md` (mcp_loop_tool_calls) · `docs/llm/flows/mcp.md` · `docs/llm/inference/job-lifecycle.md` (TurnRecord persistence) · `docs/llm/inference/session-grouping.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row TBD

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — `TurnInternalsResponse` extended with `tool_calls` field, populated by JOIN against `mcp_loop_tool_calls` | [x] | #126 | 8cfa00b |
| B — UI `api-test-conversation` displays per-round tool_calls (name/args/result/outcome/latency) | [x] v1 (broken) → [x] v2 | #126 / #127 | 8cfa00b / TBD |
| C — `bridge::run_loop` injects boundary system message at `round == max_rounds-1` if no text yet, forcing final answer | [x] (live-verified L2) | #126 | 8cfa00b |
| D — Tests: tool audit fetch + loop-convergence invariant + integration | [x] | #126 | 8cfa00b |
| CDD-sync — S3 vs PG split documented in `mcp.md` § Audit exposure | [x] v1 → [x] v2 | #126 / #127 | 8cfa00b / TBD |
| Live verify — L2 boundary log + L5 audit endpoint surfacing | [x] (L2/L5 dev) | n/a | live 2026-04-30 |
| Live verify — L6 UI inline tool chain renders without "load failed" | [ ] (v2 hotfix #127 pending) | — | — |

---

## §1 Problem (verified 2026-04-30 on dev `develop-4a5a5a4`)

### §1.1 Tool result invisible in UI

User submitted MCP request "오늘 마이크론 주가 분석" via test panel (`conv_338X2CaGOnhev7hZdxL5Q`). Bridge ran 5 MCP rounds:

| round | tool | args |
|-------|------|------|
| 0 | web_search | "마이크론 주가 오늘 분석" |
| 1 | web_search | "마이크론 테크놀로지 주가 상승 이유 오늘" |
| 2 | web_search | "마이크론 주가 최근 흐름 HBM DDR5 수혜" |
| 3 | get_weather | "New York" |
| 4 | get_datetime | "America/New_York" |

Each round's MCP execution wrote to `mcp_loop_tool_calls` table (per `flows/mcp.md` line 63 `batch_insert_tool_calls`). **But `GET /v1/conversations/{id}/turns/{job_id}/internals` returns only**:

```json
{
  "job_id": "...",
  "compressed": null,
  "vision_analysis": null
}
```

Tool-call audit data (args, result_text, outcome, latency_ms) is **never exposed** to the UI. Frontend conversation page therefore shows "결과 없음" for tool-only turns even though the bridge successfully fetched search results from MCP servers.

### §1.2 MCP loop never produces final text

The 5 rounds above all emitted tool_calls — no text content — and the loop exhausted at `MAX_MCP_ROUNDS=5`. Final response delivered to client: a single tool_call (`get_datetime`) wrapped in SSE chunks. `result_text` in `TurnRecord` is `null`. The model never converted the accumulated tool results into a final answer.

Per CDD `inference/mcp.md` line 234 references `mcp-schema.md` for the audit table. Per `flows/mcp.md`, the loop runs `for round in 0..max_rounds` — no boundary instruction is injected to force termination.

`LOOP_DETECT_THRESHOLD=3` only fires on **same `(name, args_hash)` triple-call**. Distinct args (different search queries) are not detected as a loop, so the model can use up the entire round budget on tool calls without ever producing text.

---

## §2 Root cause

### §2.1 §1.1 — `get_turn_internals` is incomplete

`conversation_handlers.rs::get_turn_internals` (line 275–356) loads the S3 `ConversationRecord`, finds the matching `TurnRecord`, and returns only `compressed` + `vision_analysis`. The handler intentionally surfaces what `TurnRecord` carries directly (`turn.compressed`, `turn.vision_analysis`) but **does not consult `mcp_loop_tool_calls`** — the very table CDD `mcp-schema.md` defines specifically as the per-tool audit log:

```sql
CREATE TABLE mcp_loop_tool_calls (
    id, mcp_loop_id, job_id, loop_round,
    server_id, tool_name, namespaced_name,
    args_json,                     -- input
    result_text,                   -- output (the data the user wants to see)
    outcome,                       -- success|error|timeout|cache_hit|circuit_open
    cache_hit, latency_ms, result_bytes,
    created_at
);
```

`bridge::batch_insert_tool_calls` writes every row. UI just doesn't have an API to read them back per-turn. Pure exposure gap.

### §2.2 §1.2 — no convergence forcing in `run_loop`

`bridge::run_loop` runs `for round in 0..max_rounds` and breaks only when:
1. `mcp_calls.is_empty()` (filter for MCP-prefixed names emptied)
2. `loop_detected` (per (name, args_hash) hits `LOOP_DETECT_THRESHOLD=3`)
3. `round_result.passthrough_streamed` (S20 mixed-delta safety)
4. `provider.submit` fails

When the model emits a different MCP tool each round (different query, different tool, different args), conditions 1, 2, 3 never trigger and the loop runs all `max_rounds` iterations. The final round result is whatever the last round emitted (more often than not, another tool_call) — never a forced final text.

Industry pattern (LangGraph `recursion_limit` + boundary prompt; OpenAI Agents SDK `tool_choice` escalation): at the last round, the agent injects a **system message constraining tool_choice to "none"** so the model has no option but to produce text using the accumulated tool results.

veronex bridge does not have this. Fix this.

---

## §3 Solution

### §3.1 Tier A — extend `TurnInternalsResponse` with `tool_calls` field

Schema (CDD `mcp-schema.md` already defines `mcp_loop_tool_calls`; this is read-side projection):

```rust
// crates/veronex/src/infrastructure/inbound/http/conversation_handlers.rs

#[derive(Debug, Serialize)]
pub struct ToolCallDetail {
    pub round: i16,
    pub server_slug: String,
    pub tool_name: String,
    pub namespaced_name: String,
    pub args: serde_json::Value,
    pub result_text: Option<String>,
    pub outcome: String,
    pub cache_hit: bool,
    pub latency_ms: Option<i32>,
    pub result_bytes: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct TurnInternalsResponse {
    pub job_id: String,
    pub compressed: Option<CompressedTurnDetail>,
    pub vision_analysis: Option<VisionAnalysisDetail>,
    /// MCP per-tool audit for this turn — joined from `mcp_loop_tool_calls`.
    /// Empty Vec when no MCP tools were invoked. Ordered by `loop_round ASC, created_at ASC`.
    pub tool_calls: Vec<ToolCallDetail>,
}
```

Handler change: add a SQL fetch in `get_turn_internals`:

```rust
let tool_calls: Vec<ToolCallDetail> = sqlx::query_as!(...)
    .bind(job_uuid)
    .fetch_all(&state.pg_pool)
    .await
    .unwrap_or_default();
```

JOIN against `mcp_servers` for `server_slug` (or use `namespaced_name` parser). Order by `loop_round, created_at`.

### §3.2 Tier B — UI surface

`web/app/jobs/components/api-test-conversation.tsx` already takes `messages: ConversationMessage[]`. Extend the message component to optionally render a tool_call panel for each round:

- For `assistant` message with `tool_calls`: above the (empty) content, render an expandable `<ToolCallTimeline>` block listing each call:
  - tool name (slug-stripped)
  - input args (collapsed JSON)
  - result preview (first 500 chars; expand to full)
  - outcome badge (success/error/cache_hit/circuit_open)
  - latency_ms

Data source: when the conversation page renders a turn, fetch `/v1/conversations/{conv_id}/turns/{job_id}/internals` (already exists) and use the new `tool_calls` array.

### §3.3 Tier C — convergence forcing

Modification to `bridge::run_loop`:

```rust
for round in 0..max_rounds {
    // ── New §3.3 — convergence boundary on the final round ────────────────
    // If we are about to dispatch the LAST allowed round and the loop has
    // produced no text yet, inject a system message that constrains the
    // model to text-only output using the tool results already accumulated.
    // Pattern: LangGraph recursion_limit + boundary prompt; OpenAI Agents
    // SDK tool_choice="none" escalation.
    if round == max_rounds - 1 && content.is_empty() && rounds > 0 {
        messages.push(serde_json::json!({
            "role": "system",
            "content": "You have reached the final response step. \
                Do NOT call any more tools. Using the tool results above, \
                produce the user's final answer in natural language now."
        }));
        info!(round, "MCP convergence: final-round system message injected");
    }
    // ── /Tier C ────────────────────────────────────────────────────────

    let job_id = match state.use_case.submit(SubmitJobRequest { ... }).await { ... };
    // ... rest unchanged
}
```

Effect:
- After the boundary system message, the next (last) round is the model's final chance. Most instruction-tuned models honor "no more tools" + "produce answer now".
- If the model still emits tool_calls (degenerate case), the loop ends at `max_rounds` with whatever the last round produced; the bridge's `streamed_via_tap` (S20) plus the new `tool_calls` audit (Tier A) means the UI still shows the full chain — the user is informed of the failure mode rather than seeing a silent "결과 없음".

### §3.4 Why this is "complete" (no compromise)

| Concern | How addressed |
|---------|---------------|
| User can't see tool results | Tier A — JOIN audit table, expose via existing internals endpoint |
| Model never converges to text | Tier C — boundary system message at last round (industry pattern) |
| If model still doesn't converge after Tier C | Tier A still shows full audit chain; user understands the failure mode; future improvement = lower `max_rounds` or stronger prompt — separate tuning |
| Schema migration | None — `mcp_loop_tool_calls` already exists per `mcp-schema.md` |
| Backward compat | `tool_calls: Vec<ToolCallDetail>` defaults to empty — old clients ignore extra field |

---

## §4 Files

| File | Change |
|---|---|
| `crates/veronex/src/infrastructure/inbound/http/conversation_handlers.rs` | (a) New `ToolCallDetail` struct. (b) Extend `TurnInternalsResponse` with `tool_calls: Vec<ToolCallDetail>`. (c) `get_turn_internals` adds `sqlx::query_as!` against `mcp_loop_tool_calls` for the job_uuid, ordered by `loop_round, created_at`. |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs::run_loop` | Inject boundary system message before the final iteration when `round == max_rounds-1`, `rounds > 0`, and `content.is_empty()`. |
| `web/app/jobs/components/api-test-conversation.tsx` | Render `<ToolCallTimeline>` for tool-call turns using the new `internals.tool_calls` array. |
| `web/app/jobs/components/tool-call-timeline.tsx` (NEW) | Component: per-call name + args (JSON tree) + result preview (expandable) + outcome badge + latency. |
| `web/lib/queries/turn-internals.ts` (or similar) | Hook to fetch `/v1/conversations/{id}/turns/{job_id}/internals`; expose new `tool_calls` field. |
| `docs/llm/inference/mcp.md` | "Audit exposure" subsection: GET internals returns `tool_calls`; reference SDD. |
| `docs/llm/inference/job-api.md` | Turn-internals response schema updated. |

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | `get_turn_internals` returns `tool_calls=[]` when no MCP calls were made | conversation_handlers integration |
| 2 | `get_turn_internals` returns N entries when N tool calls were inserted; entries ordered by `loop_round` ASC | conversation_handlers integration |
| 3 | `ToolCallDetail` correctly carries `result_text`, `outcome="success"`, `latency_ms` | conversation_handlers unit |
| 4 | Bridge `run_loop` injects boundary system message exactly when `round == max_rounds-1 && rounds > 0 && content.is_empty()` | bridge unit |
| 5 | Convergence sentinel: loop where all rounds emit tool_calls + boundary message injected → assertion that the final round's input messages contain the boundary system message | bridge integration with mock model |
| 6 | (regression) all existing 34 bridge tests pass unchanged | bridge unit |

---

## §6 Live verification (dev cluster)

### §6.1 Setup
- Image rolled out to `develop-<this PR sha>`
- `qwen3-coder-next-200k:latest` warm

### §6.2 PASS conditions

| # | Check |
|---|---|
| L1 | Submit "오늘 마이크론 주가 분석" with `use_mcp:true` |
| L2 | Bridge log: at most `max_rounds-1` rounds with `mcp_calls=N`, then either text content emitted OR final round with `MCP convergence: final-round system message injected` log line |
| L3 | If model honors boundary instruction: final round emits text → `result_text` non-empty in TurnRecord; UI conversation shows answer |
| L4 | If model ignores boundary (degenerate): UI still surfaces all 5 rounds of tool calls via Tier A audit endpoint — user sees the full chain |
| L5 | `GET /v1/conversations/{id}/turns/{job_id}/internals` for a tool-call turn returns `tool_calls` with at least 1 entry containing populated `result_text` |
| L6 | UI api-test-conversation page renders tool-call timeline with input args and search results visible |

---

## §7 CDD sync (post-impl)

| File | Action |
|---|---|
| `docs/llm/inference/mcp.md` | Append "Audit exposure" subsection — `GET /v1/conversations/{id}/turns/{job_id}/internals` returns `tool_calls` array sourced from `mcp_loop_tool_calls`. |
| `docs/llm/inference/mcp.md` | Append "Loop convergence" — boundary system message at `round == max_rounds-1` when no text yet; reference industry pattern (LangGraph recursion_limit + boundary prompt; OpenAI Agents SDK tool_choice escalation). |
| `docs/llm/inference/job-api.md` | Update turn-internals response schema with `tool_calls` field. |
| `docs/llm/flows/mcp.md` | Add a step in the `run_loop` ASCII diagram: "if final round && no text → inject boundary system message". |

---

## §8 Out of Scope

- Lowering `MAX_MCP_ROUNDS` (currently 5) — separate tuning task.
- Stricter `LOOP_DETECT_THRESHOLD` reduction — current 3 is conservative; revisit only if production sees more degenerate cases.
- Per-server MCP tool result truncation policy in audit response — current full `result_text` exposure (TEXT column) is fine for dashboard view.
- Streaming `tool_calls` field in real-time during the bridge loop (server-push to UI mid-stream) — current model: UI fetches internals after turn completes. Real-time streaming is a separate UX SDD.
- Dedicated UI to inspect `mcp_loop_tool_calls` rows globally (cross-conversation analytics) — separate dashboard SDD.

---

## §10 v1 → v2 hotfix (PR #127)

After live verification on dev `develop-8cfa00b`, the test panel still
showed "load failed" for tool-only turns. Root cause:

- v1 derived the assistant message's `jobId` from the SSE `chunk.id`
  (`chatcmpl-mcp-<uuid>`). But that `<uuid>` is a synthetic stream
  identifier minted in `openai_handlers.rs:834` via `Uuid::new_v4()` —
  NOT the inference_jobs row id. The `<TurnInternals>` component then
  hit `/v1/conversations/{id}/turns/{wrong_uuid}/internals` and got 404
  → React Query `isError` → UI rendered `common.error` ("Failed to load
  data" / "데이터를 불러오지 못했습니다") — perceived by the user as
  "load failed".

- The architectural assumption was also wrong: I treated PG
  `mcp_loop_tool_calls` as the primary source for the user-visible
  tool chain, when the SSOT for turn output (model-emitted tool_calls)
  is **S3 `ConversationRecord.turns[].tool_calls_json`** — the same
  storage used by `/v1/dashboard/jobs/{id}` for "(Tool Calls)" rendering.

v2 fix:

- After the SSE stream ends and `hasMcpTools` is true, the test panel
  fetches `/v1/conversations/{convId}` and reads the **newest turn**
  from the response. That turn's real `job_id` (from S3 TurnRecord) is
  stored on the `ConversationMessage`; its `tool_calls` array is also
  stored as `toolCalls` for inline rendering.
- The assistant bubble now renders the S3-sourced tool_calls (name + args)
  inline, mirroring the pattern in `conversation-list.tsx`. Tool-only
  turns get an explicit "tool-only turn" hint where text would be.
- `<TurnInternals>` (PG audit panel) is rendered **lazily** — no
  `defaultOpen`. User clicks to expand and only then does the PG fetch
  fire. With a real `job_id`, the response is 200 (even if tool_calls
  is empty for late rounds — see §10.1).

### §10.1 Pre-existing PG persistence gap (separate scope)

Live verification surfaced an unrelated gap: when the bridge runs N
rounds, only round 0's `mcp_loop_tool_calls` rows get persisted; rounds
1..N-1 silently fail their `batch_insert_tool_calls` (no error log).
Same pattern in S22 repro data (`conv_338X2C...`) → predates S23.
Tracked separately as a follow-up SDD (S24); does NOT block this PR
because S3 turn data (the user-visible chain via v2) is unaffected.

---

## §9 References

- `docs/llm/inference/mcp.md` — MCP integration overview
- `docs/llm/inference/mcp-schema.md` — `mcp_loop_tool_calls` table schema (audit log)
- `docs/llm/flows/mcp.md` — `run_loop` ASCII flow including `batch_insert_tool_calls`
- `docs/llm/inference/job-lifecycle.md` — TurnRecord / S3 ConversationRecord
- LangGraph recursion_limit + boundary prompt pattern (industry reference)
- OpenAI Agents SDK `tool_choice="none"` escalation (industry reference)
- `.specs/veronex/history/inference-mcp-streaming-first.md` — streaming-first SDD (related)
- `.specs/veronex/bridge-mcp-loop-correctness.md` (S20) — fast-path drop, stream-tap (companion)
