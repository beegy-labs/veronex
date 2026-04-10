# MCP (Model Context Protocol) Integration

> SSOT | **Last Updated**: 2026-04-08

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
YQL: where environment = "prod" and tenant_id = "default"
     and ({targetHits: K}nearestNeighbor(embedding, qe))
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

---

## Known Limitations (Future Work)

| Issue | Impact | Planned Fix |
|-------|--------|-------------|
| TPM reservation uses `TPM_ESTIMATED_TOKENS = 500` | MCP sessions can consume 50K+ tokens; rate limiting underestimates | Reserve 4,000 tokens when `tools` present; record actual at loop end |
| No per-key MCP session concurrency limit | One key can monopolize MCP bridge | Per-key session semaphore |

---

## API Endpoints

MCP server management requires JWT Bearer auth (`settings_manage` permission).
ACL management requires `settings_manage` permission.
`/v1/mcp/targets` is internal-network only (no auth).

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/mcp/servers` | List all servers (online status + tool count) |
| `POST` | `/v1/mcp/servers` | Register a new MCP server |
| `PATCH` | `/v1/mcp/servers/{id}` | Update name / URL / enabled |
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
