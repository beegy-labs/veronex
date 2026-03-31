# MCP Agentic Loop Flow

> **Last Updated**: 2026-03-28

---

## Entry Point

```
openai_handlers::chat_completions()
  │
  ├── mcp_bridge.is_some() && should_intercept() ?
  │     should_intercept() = session_manager.has_sessions()
  │     (true when ≥1 enabled MCP server has an active session)
  │
  └── YES → mcp_ollama_chat()
              │
              ├── resolve model:
              │     lab_settings.mcp_orchestrator_model  (if set)
              │     else req.model
              │
              └── bridge.run_loop(...)
```

---

## `run_loop()` — Agentic Loop

```
run_loop(state, caller, model, messages, base_tools, want_stream)
  │
  ├── 1. Per-key MCP ACL (see flows/auth.md)
  │     API key → query mcp_key_access → Some(HashSet<server_id>)
  │     JWT     → None (bypass)
  │
  ├── 2. Build tool list
  │     tool_cache.get_all(allowed_servers)
  │       └── merge base_tools + MCP tools (cap: MAX_TOOLS_PER_REQUEST=32)
  │
  └── 3. Loop (max MAX_ROUNDS=5):
        │
        ├── submit job (use_case.submit)    ← enqueues to inference queue
        │
        ├── [want_stream && rounds > 0]?
        │     └── return final_job_id for SSE pipe (skip collect)
        │
        ├── collect_round(job_id)           ← consume token stream
        │     └── RoundResult { content, tool_calls, tokens, finish_reason }
        │
        ├── filter tool_calls for MCP names (prefix "mcp_")
        │
        ├── mcp_calls empty?
        │     └── YES → break (model answered with text or non-MCP tools)
        │
        ├── loop detection: (tool_name, args_hash) × LOOP_DETECT_THRESHOLD=3
        │     └── repeated → break early
        │
        ├── append assistant message { tool_calls } to messages
        │
        ├── execute_calls(mcp_calls)        ← buffered(MAX_CONCURRENT=8)
        │     └── for each call → execute_one()
        │
        ├── append tool result messages { role: "tool", content }
        │
        └── rounds += 1 → GOTO submit
```

---

## `execute_one()` — Single Tool Call

```
execute_one(tool_call, api_key_id, allowed_servers)
  │
  ├── resolve server_id from tool_cache (namespaced → server)
  │     └── not found → return {"error": "unknown tool"}
  │
  ├── ACL double-check: allowed_servers.contains(server_id)?
  │     └── denied → return {"error": "MCP server access denied"}
  │
  ├── circuit_breaker.is_open(server_id)?
  │     └── open → emit span(outcome=circuit_open) → return error
  │
  ├── result_cache.get(tool_def, args)?
  │     └── hit → emit span(outcome=cache_hit) → return cached
  │
  ├── timeout = server.timeout_secs (per-server config)
  │
  ├── session_manager.call_tool(server_id, raw_name, args)
  │     └── HTTP POST to MCP server /  (JSON-RPC tools/call)
  │
  ├── timeout elapsed → circuit_breaker.record_failure() → return timeout error
  │
  ├── tool result:
  │     ├── isError=false → circuit_breaker.record_success()
  │     │                 → result_cache.set(TTL=300s)
  │     │                 → truncate at MAX_TOOL_RESULT_BYTES=32768
  │     └── isError=true  → circuit_breaker.record_failure()
  │
  ├── emit OTel span (target: veronex::mcp::tool_call)
  │     → ClickHouse mcp_tool_calls_hourly (via OTel pipeline)
  │
  └── INSERT INTO mcp_loop_tool_calls (loop_id, job_id, round, outcome, latency_ms)
```

---

## Tool Naming

```
Namespaced name:  mcp_{server_slug}_{tool_name}
Example:          mcp_weather_get_weather

Stored in:        mcp_server_tools.namespaced_name
Used as:          tool["function"]["name"] exposed to the LLM
```

---

## ACL Summary

```
Caller type    │  allowed_servers value  │  Effect
───────────────┼─────────────────────────┼─────────────────────────────────
API key        │  Some({})               │  No MCP tools injected (deny all)
API key        │  Some({id1, id2})       │  Only id1, id2 servers accessible
JWT session    │  None                   │  All active servers accessible
```

---

## Loop Protections

| Protection | Value | Behavior |
|-----------|-------|----------|
| Max rounds | 5 | Hard loop limit |
| Loop detect threshold | 3 | Same (tool, args_hash) ×3 → break |
| Per-round timeout | 120s | `COLLECT_ROUND_TIMEOUT` |
| Max concurrent tool calls | 8 | `buffered(8)` in execute_calls |
| Max tool result size | 32 KB | Truncated before injection |
| Max tools per request | 32 | Context window protection |
| Result cache TTL | 300s | Idempotent tool calls |

---

## Background: Tool Refresh Loop (main.rs)

```
25s interval → tool_cache L2 refresh from Valkey
  keeps Valkey cache warm before 35s TTL expiry
  no HTTP calls — reads existing Valkey keys only
```

## Files

| File | Purpose |
|------|---------|
| `infrastructure/outbound/mcp/bridge.rs` | `McpBridgeAdapter` — full ReAct loop |
| `infrastructure/inbound/http/openai_handlers.rs` | Entry, `should_intercept()`, `mcp_ollama_chat()` |
| `infrastructure/inbound/http/mcp_handlers.rs` | MCP server CRUD, `discover_and_persist_tools()` |
| `infrastructure/inbound/http/key_mcp_access_handlers.rs` | ACL management REST API |
| `veronex-mcp/src/tools/` | MCP tools (get_weather, web_search) |
| `veronex-agent/src/mcp_discover.rs` | Periodic tool discovery + embedding |
| `veronex-embed/src/` | Embedding service (multilingual-e5-large) |
