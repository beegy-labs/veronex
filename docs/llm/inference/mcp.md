# MCP (Model Context Protocol) Integration

> SSOT | **Last Updated**: 2026-04-28

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

When `should_intercept()` selects the MCP path (`openai_handlers.rs::chat_completions`), the response is **always** Server-Sent Events regardless of the client's `stream` field. Multi-round agentic loops have unbounded variance (each round ~30 s × up to `MAX_ROUNDS=5`); a single bundled HTTP body cannot fit under Cloudflare's 100 s origin idle-timeout. Server-driven SSE keeps the connection alive via 15 s `KeepAlive` heartbeats throughout the loop's variance window.

Implementation:

| Concern | Mechanism |
|---------|-----------|
| Headers | `sse_response()` (`handlers.rs`) attaches `Content-Type: text/event-stream`, `Cache-Control: no-cache, no-transform`, `Connection: keep-alive`, `X-Accel-Buffering: no` |
| Heartbeat | `axum::response::sse::KeepAlive::new().interval(SSE_KEEP_ALIVE)` (15 s) |
| Response is constructed BEFORE bridge completes | `mcp_ollama_chat` spawns `bridge.run_loop` on `tokio::spawn`; SSE stream awaits result via `tokio::sync::oneshot`. axum flushes 200 + headers + first heartbeat within ms of the request |
| OpenAI-compat shape | `chat.completion.chunk` events with `delta.content` / `delta.tool_calls`; final `[DONE]` sentinel |
| Cancel-on-disconnect | spawned bridge task runs to completion (best-effort detached); `runner::persist_partial_conversation` writes partial state to S3 for each affected round |
| S3 ConversationRecord | Runner writes one `TurnRecord` per round, keyed by that round's `job_id` (`conversations/{owner_id}/{conversation_id}.json.zst` is the conversation-scoped append target). Bridge no longer writes S3 — only updates loop-wide token totals on `first_job_id` and deletes intermediate-round DB rows. SDD: `.specs/veronex/history/inference-mcp-per-round-persist.md` §3. |

Verified live 2026-04-29 — 240 s response held alive (4 min, > 2× Cloudflare timeout); no 524 observed; final answer streamed in 195 tokens. Note: §9.5 of the streaming-first SDD recorded this as PASS based on SSE output only; the dashboard detail GET's `result_text` non-empty assertion was added in `.specs/veronex/history/inference-mcp-per-round-persist.md` §8.

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
| Session self-heal | `reconcile_mcp_sessions()` reconnects missing sessions every 25 s — see Session Lifecycle |

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
