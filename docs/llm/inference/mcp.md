# MCP (Model Context Protocol) Integration

> SSOT | **Last Updated**: 2026-03-25

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
           │ should_intercept() → true (tools present + MCP servers enabled for caller)
           ▼
    mcp_ollama_chat()
           │
           ├── reads lab_settings.mcp_orchestrator_model (override or req.model)
           │
           ▼
    McpBridgeAdapter.run_loop()
           │
           ├── Round 1: POST Ollama /api/chat (orchestrator model)
           │     model response → tool_calls: [mcp_server_slug_tool_name, ...]
           │
           ├── execute_calls() → join_all per tool call
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
| `openai_handlers.rs` | `chat_completions()` dispatch + `mcp_ollama_chat()` |
| `infrastructure/outbound/mcp/bridge.rs` | `McpBridgeAdapter` — `run_loop()`, `execute_calls()`, `execute_one()` |
| `infrastructure/outbound/mcp/session_manager.rs` | Per-server MCP session lifecycle, tool discovery |
| `application/ports/outbound/mcp_repository.rs` | `McpServerRepository` port |
| `infrastructure/inbound/http/mcp_handlers.rs` | REST CRUD for MCP server management |

---

## `should_intercept()`

MCP interception triggers when **all** of the following are true:

1. `provider_type == "ollama"` (only Ollama-backed requests)
2. `tools` array is present and non-empty in the request
3. At least one enabled MCP server exists (`mcp_bridge.is_some()`)
4. The caller has MCP access (`mcp_key_access` row exists, or session user)

---

## Orchestrator Model

The model used for MCP tool-call loops is determined in this order:

1. `lab_settings.mcp_orchestrator_model` (if set) — global override for all MCP requests
2. `req.model` (fallback) — the model the client specified

This allows ops to pin a well-suited model (e.g. `qwen3:8b`) cluster-wide without
requiring clients to change their requests.

Recommended model for multilingual (Korean/English/Japanese) workloads: **`qwen3:8b`**
- 128K context window
- Hermes tool-calling format (structured, reliable)
- Native CJK support
- Strong tool-call restraint (does not over-call tools)

Configured via: `PATCH /v1/dashboard/lab` → `mcp_orchestrator_model`
UI: MCP page → Orchestrator Model card

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

## DB Schema

```sql
-- MCP server registry
CREATE TABLE mcp_servers (
    id           UUID        PRIMARY KEY DEFAULT uuidv7(),
    name         VARCHAR(128) NOT NULL UNIQUE,
    slug         VARCHAR(64)  NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9_]+$'),
    url          TEXT         NOT NULL,
    is_enabled   BOOLEAN      NOT NULL DEFAULT true,
    timeout_secs SMALLINT     NOT NULL DEFAULT 30 CHECK (timeout_secs BETWEEN 1 AND 300),
    metadata     JSONB        NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Tool capability snapshot (cache from tools/list)
CREATE TABLE mcp_server_tools (
    server_id       UUID  NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name       TEXT  NOT NULL,
    namespaced_name TEXT  NOT NULL,  -- "mcp_{slug}_{tool_name}"
    description     TEXT,
    input_schema    JSONB NOT NULL DEFAULT '{}',
    annotations     JSONB NOT NULL DEFAULT '{}',
    discovered_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, tool_name)
);

-- Per-API-key access control (default deny; insert row to grant)
CREATE TABLE mcp_key_access (
    api_key_id UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    server_id  UUID    NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    is_allowed BOOLEAN NOT NULL DEFAULT true,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (api_key_id, server_id)
);
```

Migration: `000011_mcp_capabilities.up.sql`

---

## Protections (Current)

| Protection | Implementation |
|-----------|----------------|
| Per-tool timeout | `MCP_TOOL_CALL_TIMEOUT = 30s` (domain constants) |
| Circuit breaker | Per-server, trips on consecutive failures |
| Result cache | 300s TTL per tool call (idempotent tools) |
| Max result size | `MAX_TOOL_RESULT_BYTES = 32768` — truncates oversized results |
| Max rounds | `MAX_LOOP_ROUNDS` — prevents infinite tool-call loops |

---

## Known Limitations (Future Work)

| Issue | Impact | Planned Fix |
|-------|--------|-------------|
| `execute_calls()` uses `join_all` — no concurrency limit | Under high load: N×M simultaneous MCP server calls | Per-server `tokio::sync::Semaphore` (recommended limit: 10) |
| TPM reservation uses `TPM_ESTIMATED_TOKENS = 500` | MCP sessions can consume 50K+ tokens; rate limiting underestimates | Reserve 4,000 tokens when `tools` present; record actual at loop end |
| No per-key MCP session concurrency limit | One key can monopolize MCP bridge | Per-key session semaphore |

---

## API Endpoints

All MCP management endpoints require JWT Bearer auth.
Handler: `crates/veronex/src/infrastructure/inbound/http/mcp_handlers.rs`

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/mcp/servers` | List all servers (paginated) |
| `POST` | `/v1/mcp/servers` | Register a new MCP server |
| `PATCH` | `/v1/mcp/servers/{id}` | Update name / URL / timeout / enabled |
| `DELETE` | `/v1/mcp/servers/{id}` | Remove server + cascade tools + access rows |
| `GET` | `/v1/mcp/servers/{id}/tools` | List discovered tools for a server |

---

## Frontend

Page: `/mcp` → `web/app/mcp/page.tsx` → renders `<McpTab />`
Component: `web/app/providers/components/mcp-tab.tsx`

Sections:
1. **Orchestrator Model** card — `OrchestratorModelSelector` (reads `GET /v1/dashboard/lab`, patched via `PATCH /v1/dashboard/lab`)
2. **Register Server** button → `RegisterMcpModal` dialog
3. **Server table** — name, slug, URL, online status, tool count, enabled toggle, delete
