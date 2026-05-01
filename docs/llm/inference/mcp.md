# MCP (Model Context Protocol) Integration

> SSOT | **Last Updated**: 2026-05-01

Veronex acts as an **MCP client** — it connects to external MCP servers and
executes their tools on behalf of LLM inference loops.
MCP servers do NOT call Ollama; the API server (veronex) handles all Ollama calls.

---

## Architecture

```
Client → POST /v1/chat/completions
           │
           ▼
    openai_handlers.rs
    chat_completions()
           │ should_intercept() → true (≥1 active MCP session)
           ▼
    mcp_ollama_chat()
           │
           ▼
    McpBridgeAdapter.run_loop()
           │
           ├── Round 1: POST Ollama /api/chat (orchestrator model)
           │     model response → tool_calls: [mcp_server_slug_tool_name, ...]
           │
           ├── execute_calls() → buffered(8) per tool call
           │     └── execute_one() → circuit breaker → result cache → session_manager.call_tool()
           │           └── HTTP call to MCP server
           │
           ├── Append tool results → messages[]
           │
           └── Round N: repeat until model produces text (no tool_calls) or MAX_ROUNDS
```

**Key invariant**: Ollama is always called from `run_loop()` inside `openai_handlers.rs`.
MCP servers receive only tool invocations — they never receive inference requests.

---

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | `chat_completions()` dispatch + `mcp_ollama_chat()` |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | `McpBridgeAdapter` — `run_loop()`, `execute_calls()`, `execute_one()` |
| `crates/veronex-mcp/src/session.rs` | Per-server MCP session lifecycle + re-init on 404 |
| `crates/veronex-mcp/src/tool_cache.rs` | Two-level (L1 DashMap + L2 Valkey) tool schema cache |
| `crates/veronex/src/infrastructure/inbound/http/mcp_handlers.rs` | REST CRUD for MCP server management |
| `crates/veronex/src/infrastructure/inbound/http/key_mcp_access_handlers.rs` | Per-key ACL grant/revoke |

---

## `should_intercept()`

`McpBridgeAdapter::should_intercept()` returns `true` when at least one MCP server
has an active session (`session_manager.has_sessions()`).

The caller (`openai_handlers.rs`) additionally checks:
- `provider_type == "ollama"` (MCP loop only supported for Ollama)
- `mcp_bridge.is_some()` (feature enabled at startup)

ACL filtering happens inside `run_loop()` after interception — API-key callers with
no granted servers receive an empty tool list (default deny).

---

## Session Lifecycle

Sessions are per-replica (the `Mcp-Session-Id` header is not shared across pods).

| Phase | Trigger | Effect |
|-------|---------|--------|
| Startup | `main.rs` reads `mcp_servers WHERE is_enabled = true` and calls `session_manager.connect()` for each | Session stored in `DashMap<server_id, SessionEntry>` |
| Per-request 404 | `with_session()` sees `SESSION_EXPIRED_MARKER` | Per-server mutex + re-init + retry once |
| **Periodic reconcile (25 s)** | `reconcile_mcp_sessions()` in the refresh loop | Reconnects any enabled server missing a session, then runs `discover_tools_startup()` to populate L1 + Vespa |

Without the reconcile step, a transient boot-time `connect()` failure (gateway
cold-start race, brief upstream outage) leaves `should_intercept()` false for
the lifetime of the pod — every chat completion silently bypasses MCP. The
periodic reconcile makes the bridge self-healing without a pod restart.

---

## Tool Naming Convention

MCP tools are namespaced to prevent collisions:

```
mcp_{server_slug}_{tool_name}
```

Example: server slug `search`, tool `web_search` → `mcp_search_web_search`

The `namespaced_name` is stored in `mcp_server_tools.namespaced_name` and used
as the tool name exposed to the LLM.

---

---

## Vector Tool Selection (Vespa)

When `VESPA_URL` + `EMBED_URL` are configured, `McpVectorSelector` replaces the full `get_all()` fallback with semantic ANN search.

```
User query → embed (veronex-embed) → Vespa ANN → Top-K tools → LLM
                    ↑
           Valkey cache (5 min TTL, keyed by SHA256[:16] of query)
```

### Vespa Document Structure

```
tool_id     = "{environment}:{tenant_id}:{server_id}:{tool_name}"
environment = VESPA_ENVIRONMENT (environment-level partition: prod, dev, local-dev)
tenant_id   = VESPA_TENANT_ID  (tenant-level sub-partition, default: "default")
embedding   = 1024-dim float32 (multilingual-e5-large via veronex-embed)
```

### Multi-Environment Isolation

A single Vespa instance serves multiple environments (prod, dev, local-dev) simultaneously. Each environment writes and reads under its own `environment` partition:

```
YQL: where environment contains "prod" and tenant_id contains "default"
     and ({targetHits: K}nearestNeighbor(embedding, qe))
Note: string attributes use `contains`, not `=` (= is for numeric fields).
```

`environment` and `tenant_id` are injected via env vars:
- **Helm**: `veronex.vespaEnvironment` / `veronex.vespaTenantId` → `VESPA_ENVIRONMENT` / `VESPA_TENANT_ID`
- **docker-compose**: `VESPA_ENVIRONMENT=${VESPA_ENVIRONMENT:-local-dev}` / `VESPA_TENANT_ID=${VESPA_TENANT_ID:-default}`

### Indexing Lifecycle

| Event | Action |
|-------|--------|
| MCP server registered / tools discovered | `McpToolIndexer.index_server_tools(environment, tenant_id, server_id, tools)` |
| MCP server deleted | `McpToolIndexer.remove_server_tools(environment, tenant_id, server_id)` |
| Periodic refresh (25s) | Re-index if tool cache changes |

### Fallback

On any Vespa/embed error → falls back to `tool_cache.get_all()` (all registered tools, capped at `MAX_TOOLS_PER_REQUEST = 32`).

### YQL Contract

| Field | Operator | Reason |
|-------|----------|--------|
| `environment`, `tenant_id` (string attribute) | `contains` | YQL `=` is a numeric range op; hyphenated values (`local-dev`) trigger `Illegal embedded sign character` |
| Selection language (DELETE) | `==` | Selection language uses `==` for both numeric and string |

Regression test: `vespa_search_uses_contains_for_string_attributes` in `crates/veronex-mcp/src/vector/tests.rs`. Reverting to `=` makes wiremock body-match miss → test fails deterministically.

### Response framing — server-driven SSE for MCP-routed requests

When `should_intercept()` selects the MCP path (`openai_handlers.rs::chat_completions`), the response is **always** Server-Sent Events regardless of the client's `stream` field. Multi-round agentic loops have unbounded variance (each round ~30 s × up to `MAX_ROUNDS=5`); the framing keeps the connection alive via 15 s `KeepAlive` heartbeats throughout the loop's variance window.

Long-stream public access uses CF-bypass direct hostname (`*.girok.dev` DNS-only CNAME → `home-gw.girok.dev` → `cilium-gateway-web-gateway` with HTTPRoute `timeouts.request=1800s`); see `.add/domain-integration.md`. The legacy CF-proxied path (`*.verobee.com`, 100 s edge idle) is no longer the primary inference route.

The SSE wrapper in `infrastructure/inbound/http/handlers.rs::with_sse_timeout` enforces `SSE_TIMEOUT=1700s` — strictly less than the Cilium gateway 1800s — so the client always sees a clean `event: error data: stream timeout` rather than an opaque gateway 504. `INFERENCE_ROUTER_TIMEOUT=1750s` covers non-streaming requests and is held above SSE_TIMEOUT so streaming uses its inner wrapper first. The 1700s headroom accommodates worst-case multi-round MCP loops (`MAX_ROUNDS=5 × ROUND_TOTAL_TIMEOUT=1500s` per round, capped to a single full round in practice for any one client wait) plus the optional S24 synthesis round on top. Invariants pinned by `infrastructure::inbound::http::constants::timeout_invariants` tests.

Implementation:

| Concern | Mechanism |
|---------|-----------|
| Headers | `sse_response()` (`handlers.rs`) attaches `Content-Type: text/event-stream`, `Cache-Control: no-cache, no-transform`, `Connection: keep-alive`, `X-Accel-Buffering: no` |
| Heartbeat | `axum::response::sse::KeepAlive::new().interval(SSE_KEEP_ALIVE)` (15 s) |
| Response is constructed BEFORE bridge completes | `mcp_ollama_chat` spawns `bridge.run_loop` on `tokio::spawn`; SSE stream consumes a `tap_rx` (token-by-token forward channel) until the bridge drops it, then awaits the bridge `oneshot` for the final summary. axum flushes 200 + headers + first heartbeat within ms of the request |
| OpenAI-compat shape | `chat.completion.chunk` events with `delta.content` / `delta.tool_calls`; final `[DONE]` sentinel |
| Round-level synchronous collection (S20) | All rounds — including the final text round — go through `collect_round` synchronously. The legacy streaming fast-path (skip collection of round N+1 when `rounds > 0` and stream the runner job directly) was dropped because (1) it bypassed MCP detection on round N+1, breaking the loop invariant when a model emitted another tool_call, (2) its sole driving constraint (CF Edge 100 s idle) was removed by platform-gitops PRs #598/#599/#600 (CF-bypass routing). SDD: `.specs/veronex/bridge-mcp-loop-correctness.md`. |
| Token-by-token UX preserved (S20 stream-tap) | `collect_round` accepts an optional `sse_tx: UnboundedSender<String>`. On the first non-empty delta of a round: tool_calls → silent intercept (bridge executes MCP tool, runs next round); content → passthrough mode (forward this and subsequent content tokens to the SSE writer). OpenAI spec round-level XOR ensures the first delta classifies the entire round. Mixed-delta rounds (vLLM bug class — content first, tool_call later) → passthrough wins, tool_calls dropped with `warn!` log per SDD §3.3. |
| Cancel-on-disconnect | spawned bridge task runs to completion (best-effort detached); `runner::persist_partial_conversation` writes partial state to S3 for each affected round |
| S3 ConversationRecord | **Bridge is the sole writer for MCP loops** (revised 2026-05-01). Runner skips S3 writes + `update_conversation_counters` whenever `job.mcp_loop_id IS Some`; bridge persists exactly one consolidated `TurnRecord` (with `tool_calls` = every round's emitted MCP calls, `result` = final text) at loop end and bumps `conversations.turn_count` by 1. One user question with N agentic rounds therefore maps to one logical turn. UI surfaces "MCP {N}회" from the head turn's `tool_calls.length`. Per-round persistence (the prior policy from `.specs/veronex/history/inference-mcp-per-round-persist.md`) yielded `turn_count = N` for one question and is no longer in effect. |
| Phase-aware timing (S19 + S19.1) | `bridge::collect_round` uses three distinct timeouts gated by a `StreamToken::phase_boundary()` sentinel emitted by `runner` after `ensure_ready` succeeds: `LIFECYCLE_TIMEOUT=600s` (Phase 1 cold-load slack — ~2.4× the observed 248 s worst case on `qwen3-coder-next-200k:latest`), `TOKEN_FIRST_TIMEOUT=300s` (Phase 2 first token after load — bumped from 60s in S19.1 after live verify showed 60s too tight for 200K-context prefill + ~5K MCP prompt tokens; first token is **not** sub-second when prefill is large), `STREAM_IDLE_TIMEOUT=45s` (mid-stream idle), `ROUND_TOTAL_TIMEOUT=1500s` (round cap, locked strictly less than upstream Cilium HTTPRoute `timeouts.request=1800s` by `tests::round_total_under_gateway_request_timeout`). RoundError variants `LifecycleTimeout` / `FirstTokenTimeout` distinguish cold-stuck vs hung-post-load for retry decisions. `MCP_LIFECYCLE_PHASE=off` legacy path: no boundary emitted → bridge stays Phase 1 the whole round, single 600 s applies. Long-stream public access uses CF-bypass direct hostname (`*.girok.dev`) — see `.add/domain-integration.md`. SDD: `.specs/veronex/bridge-phase-aware-timing.md`. |

Verified live 2026-04-29 — 240 s response held alive (4 min, > 2× Cloudflare timeout); no 524 observed; final answer streamed in 195 tokens. Note: §9.5 of the streaming-first SDD recorded this as PASS based on SSE output only; the dashboard detail GET's `result_text` non-empty assertion was added in `.specs/veronex/history/inference-mcp-per-round-persist.md` §8.

### Shim path — ReAct fallback for non-native models

The veronex gateway pattern (vision shim is the canonical example: `inference_helpers.rs::analyze_images_for_context`) provides capability adapters so any underlying Ollama model can use a feature regardless of native fine-tuning. The MCP equivalent is the **ReAct shim**:

| Stage | Mechanism |
|---|---|
| Capability gate | `ollama::capability::heuristic_supports_native(model)` — known native-tool-calling families (Qwen3-Coder, Qwen2.5-Instruct/Coder, Llama 3.1+, Mistral-instruct-v0.3+, Hermes/Nous, Command-R, Gemma 4) → native path. Default unknown → ReAct path. Future: cache `/api/show` template inspection. |
| System prompt | `mcp::react_prompt::build_react_system_prompt(tools)` — locked `Thought:` / `Action:` / `Action Input:` / `Observation:` / `Final Answer:` template injected at `messages[0]`. Tools rendered as markdown bullets with name + description + JSON schema. |
| Submission | Job submitted WITHOUT `tools` field — model receives the catalog as system text, not native fine-tune scaffolding. |
| Output parser | `mcp::react_parser::ReActParser` — stream-aware state machine. Bracket-counting JSON extractor (string-aware escape tracking) handles multi-line / nested-JSON Action Input. Fail-opens to `Final` on parse error or empty trailing text. |
| Action execution | Reuses the native path's `execute_calls` machinery — circuit breaker, ACL, result cache, analytics, observability spans all apply uniformly. |
| Observation feedback | Tool result appended back into `messages[]` as `{"role":"user","content":"Observation: ..."}` along with the assistant's serialized `Action:` line. |
| Loop detection | Same `(name, args_hash)` × `LOOP_DETECT_THRESHOLD` rule as native — terminates ReAct loop on repeated identical calls. |
| Termination | `Final Answer:` payload becomes `McpLoopResult.content`. Token totals roll up to `first_job_id`; intermediate-round DB rows cleaned up identically to native. |

**Honest limitation** (per SDD §10): the shim is an infrastructure hook, not a model-capability lift. 8B-parameter agents routinely ignore both native `tool_calls` AND ReAct text patterns when prompts get complex (per Medium / Qwen team docs). Real value: mid-size long tail (Mistral-7B-Instruct-v0.2, Llama 3 base, community fine-tunes that emit text patterns reliably but lack native tool_calls fine-tuning).

**Live verified 2026-04-29** (image `develop-81c8f57`):
- `qwen3:8b` request → routed to ReAct shim (log: `MCP: routing to ReAct shim (non-native tool_calls model)`); model itself ignored the prompt and returned plain text — confirmed model-capability limit, infrastructure intact.
- `qwen3-coder-next-200k:latest` → native path unchanged (no ReAct routing log; `MCP round complete round=0 mcp_calls=1` confirmed `web_search` invoked).

SDD: `.specs/veronex/history/mcp-react-shim.md`.

### Phase 1 Lifecycle / Phase 2 Inference

Behind feature flag `MCP_LIFECYCLE_PHASE` (default `false`), `runner::run_job`
splits provider work into two distinct phases — see `flows/model-lifecycle.md`
for the full state machine.

| Phase | Method | Effect |
|-------|--------|--------|
| 1 — Lifecycle | `provider.ensure_ready(model)` | Probes load (warm hit / coalesce / cold-load via zero-prompt `POST /api/generate`); updates VramPool |
| 2 — Inference | `provider.stream_tokens(&job)` | Token streaming, only after Phase 1 success |

`LlmProviderPort: InferenceProviderPort + ModelLifecyclePort` (blanket impl in
`application/ports/outbound/inference_provider.rs`) lets call sites hold one
trait object and drive both phases. `OllamaAdapter` implements both;
`GeminiAdapter` ships a no-op `ModelLifecyclePort` (cloud — `AlreadyLoaded`).

When the flag is **off**, behaviour is byte-identical to pre-Tier-C — implicit
auto-load remains inside `stream_tokens`. Bridge phased timeouts (PR #90)
stay as defense-in-depth on both paths. SDD: `.specs/veronex/history/inference-lifecycle-sod.md`.

### Verification (2026-04-28)

End-to-end ReAct verified on `veronex-api-dev.verobee.com` after YQL fix (#88):

| Property | Pre-fix | Post-fix |
|----------|---------|----------|
| `WARN McpVectorSelector: search failed` | every chat completion | none |
| MCP rounds for single-tool query | 3 rounds (fallback select all 4) | 1 round (top-K vector match) |
| Tool result citation | ignored — model hallucinated | quoted verbatim |
| Missing-tool honesty | fabricated values | "데이터 없음" / refused |

→ `mcp-schema.md` — DB schema (mcp_servers, mcp_server_tools, mcp_key_access, mcp_loop_tool_calls)

---

## Protections (Current)

| Protection | Implementation |
|-----------|----------------|
| Per-tool timeout | `server.timeout_secs` (configurable per server, default 30s) |
| Circuit breaker | Per-server, trips on consecutive failures |
| Result cache | 300s TTL per tool call (idempotent + read-only tools) |
| Max result size | `MAX_TOOL_RESULT_BYTES = 32768` — truncates oversized results |
| Max rounds | `MAX_ROUNDS = 5` — prevents infinite tool-call loops |
| Concurrent calls | `buffered(8)` — max 8 tool calls in-flight per round |
| Max tools per request | `MAX_TOOLS_PER_REQUEST = 32` — context window cap |
| Loop detection | Same `(tool, args_hash)` ×3 triggers early break |
| Convergence boundary | At `round + 1 == MAX_ROUNDS`, if `rounds > 0` and no text content yet, `run_loop` (a) injects a system message instructing text-only output AND (b) **omits the `tools` schema** from that final-round submit. Both halves are required because Ollama's OpenAI-compat endpoint silently drops `tool_choice` (Ollama issue [#8421](https://github.com/ollama/ollama/issues/8421), open request [#11171](https://github.com/ollama/ollama/issues/11171)) — removing schemas entirely is the only reliable way to suppress tool emission on Ollama. The system-message-only variant fails on tool-eager models like qwen3-coder (Qwen [#475](https://github.com/QwenLM/Qwen3-Coder/issues/475)). Tool *results* (`role:"tool"`) remain in the messages array so the model can still synthesize. |
| Synthesis round (S24) | If the round-loop exhausts with `content` still empty and `rounds > 0`, `run_loop` dispatches one extra **synthesis round** on a fresh messages array `[system_directive, user_prompt, system_with_tool_results]` — no `assistant.tool_calls` history, no `tools` schema. The boundary's tools-omission alone is insufficient because Qwen3-Coder learns the `<tool_call>` token pattern from prior `assistant.tool_calls` entries left in history (kept intact for OpenAI `tool_call_id` ↔ tool-result invariant) and reproduces them from training; the synthesis round eliminates that signal by giving the model nothing to mimic. SDD `.specs/veronex/mcp-synthesis-round.md`. |
| Session self-heal | `reconcile_mcp_sessions()` reconnects missing sessions every 25 s — see Session Lifecycle |

---

## Audit exposure

**S3 is the single source for the conversation chain — including per-tool execution audit.** PG carries no per-tool audit body anymore; ClickHouse retains analytics aggregates via `fire_mcp_ingest`.

| Storage | What it carries | Writer |
|---|---|---|
| **S3** `ConversationRecord.turns[]` | One `TurnRecord` per logical user question. `tool_calls[]` is an enriched array — every round's `{function:{name, arguments}, round, server_slug, result, outcome, cache_hit, latency_ms, result_bytes}`. `result` is the MCP server's response body (capped at `MAX_TOOL_RESULT_BYTES = 32 KB`). | Bridge (single-writer for MCP loops, see `bridge.rs::run_loop` end block). Runner skips S3 + `update_conversation_counters` whenever `job.mcp_loop_id IS Some`. |
| **ClickHouse** `mcp_tool_calls` | Per-tool analytics signal (call count, success rate, latency, cache hit ratio, bytes). Drives `/v1/mcp/stats` and the dashboard cards. | Bridge → `fire_mcp_ingest` → `veronex-analytics` → CH (unchanged). |
| **PG** `mcp_loop_tool_calls` | **Retired 2026-05-01.** Was duplicating 32 KB result bodies under PGLZ when zstd in S3 compresses ~3× better, plus 220 B per-row overhead × three indexes. UI cut over to S3 single-fetch — endpoint `GET /v1/conversations/{id}/turns/{job_id}/internals` no longer carries `tool_calls` (returns empty array for backwards-compatible TS clients). | — |

UI consumption (single fetch):

- **Primary chain** — `GET /v1/conversations/{id}` reads the S3 record. Each `turns[].tool_calls[]` entry renders inline in the assistant bubble: tool name, round, outcome pill, latency, args, and a `<details>` block for the full `result` body.
- Conversation header shows `{turn_count} 턴 · MCP {count}회 · …`, where `count = sum(turn.tool_calls.length)`.
- `<TurnInternalsPanel>` lazy expand still surfaces compression / vision metadata badges (compressed turn summaries, vision analysis token usage).

Why split into S3 + CH (no PG)?
1. S3 zstd ratio (3–5× JSON) beats PG TOAST PGLZ (~2× text); inter-document compression on the conversation file shares dictionary between rounds → ~30 KB compressed for 5 rounds × 32 KB raw vs ~85 KB on PG disk + indexes.
2. CH already owns aggregations (cardinality, percentiles, time-series); PG audit was duplicating the rare per-row lookup case.
3. UI gets one round trip (`/v1/conversations/{id}`) instead of two (`+ /internals` for tool body) — Valkey-cached for 300 s.

SDD: `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` (S3-single-source revision).

### Single-writer policy + audit consolidation (PR #134/#135, 2026-05-01)

Three drift points reconciled:

1. **FK CASCADE → DROP** (PR #134, superseded): the original audit-row durability fix flipped `mcp_loop_tool_calls.job_id` to `ON DELETE SET NULL`. PR #135 follow-up dropped the table entirely once S3 took over.
2. **Single S3 writer**: runner skips S3 turn write + `update_conversation_counters` whenever `job.mcp_loop_id IS Some`; bridge owns one consolidated `TurnRecord` write at loop end (covers both native `run_loop` and ReAct `run_loop_react`). Result: 1 user question with N rounds → `turn_count = 1`, `turn.tool_calls.length = N`.
3. **Audit body relocation**: bridge no longer batches inserts into `mcp_loop_tool_calls`; per-round `(result_text, ToolCallRecord)` is rewritten into the enriched `all_mcp_tool_calls` array that lands on the S3 turn. PG `mcp_loop_tool_calls` table dropped via idempotent `DROP TABLE IF EXISTS` at the top of `init.sql`.

Migration: helm `pre-upgrade` hook applies the DROP block on every install/upgrade. Dev DB dropped manually 2026-05-01 to verify.

---

## Known Limitations (Future Work)

| Issue | Impact | Planned Fix |
|-------|--------|-------------|
| TPM reservation uses `TPM_ESTIMATED_TOKENS = 500` | MCP sessions can consume 50K+ tokens; rate limiting underestimates | Reserve 4,000 tokens when `tools` present; record actual at loop end |
| No per-key MCP session concurrency limit | One key can monopolize MCP bridge | Per-key session semaphore |

---

## API Endpoints

MCP server management and per-key ACL management both require JWT Bearer auth
with the **`mcp_manage`** permission (added in rev 7 of `auth/jwt-sessions.md`
to decouple MCP delegation from the broader `settings_manage` /
`provider_manage` axes). `/v1/mcp/targets` is internal-network only (no auth).

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/mcp/servers` | List all servers (online status + tool count) |
| `POST` | `/v1/mcp/servers` | Register a new MCP server |
| `POST` | `/v1/mcp/servers/verify` | Probe `{url}/health` (5 s timeout) — pre-register connectivity check |
| `PATCH` | `/v1/mcp/servers/{id}` | Update name / slug / URL / enabled — slug change reconnects session and renames `mcp_{slug}_{tool}` |
| `DELETE` | `/v1/mcp/servers/{id}` | Remove server + cascade tools + access rows |
| `GET` | `/v1/mcp/stats` | Per-server, per-tool call stats (ClickHouse; `?hours=N`) |
| `GET` | `/v1/mcp/targets` | Agent discovery — enabled servers `[{id, url}]` |
| `GET` | `/v1/keys/{key_id}/mcp` | List MCP server access for a key |
| `POST` | `/v1/keys/{key_id}/mcp` | Grant a key access to a server |
| `DELETE` | `/v1/keys/{key_id}/mcp/{server_id}` | Revoke access |

---

## Frontend

Page: `/mcp` → `web/app/mcp/components/mcp-tab.tsx` → renders `<McpTab />`

Sections:
1. **Register Server** button → `RegisterMcpModal` dialog
2. **Server table** — name, slug, URL, online status, tool count (clickable → tool list dialog), enabled toggle, delete
3. **Stats card** — per-server per-tool call counts, success rate, cache hit %, avg latency (grouped by MCP server)
