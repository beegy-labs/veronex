# MCP Agentic Loop Flow

> **Last Updated**: 2026-04-28

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
              └── bridge.run_loop(...)
```

---

## `run_loop()` — Agentic Loop

```
run_loop(state, caller, model, messages, base_tools, want_stream)
  │
  ├── 1. Per-key ACL + cap_points + top_k — parallel via tokio::join!()
  │     API key → join!(fetch_mcp_acl, fetch_mcp_cap_points, fetch_mcp_top_k)
  │               acl       → Some(HashSet<server_id>)  (empty = deny all)
  │               cap_points → 0 = MCP disabled → return None
  │                            N = max rounds (min of N, MAX_ROUNDS)
  │               top_k     → Vespa ANN limit override (None = global default)
  │     JWT     → None / MAX_ROUNDS / None (bypass all)
  │
  ├── 2. Build tool list
  │     tool_cache.get_all(allowed_servers)
  │       └── merge base_tools + MCP tools (cap: MAX_TOOLS_PER_REQUEST=32)
  │
  └── 3. Loop (max MAX_ROUNDS=5):
        │
        ├── [round + 1 == max_rounds && rounds > 0 && content.is_empty()]?
        │     └── (1) inject system message: "final response step — tools
        │             are no longer available — produce final answer now"
        │     └── (2) submit with `tools: None` (omit schema entirely)
        │         Ollama drops `tool_choice` silently (#8421/#11171), so
        │         schema-removal is the only reliable text-forcing knob.
        │         Tool *results* stay in messages → model can synthesize.
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
        │     └── for each call → execute_one() → (result_text, ToolCallRecord)
        │
        ├── batch_insert_tool_calls()       ← single unnest INSERT for all N calls
        │
        ├── append tool result messages { role: "tool", content }
        │
        ├── [rounds >= 2]? prune_tool_messages(keep_last=2)
        │     └── compress tool messages older than 2 rounds to placeholder
        │         → bounds context window growth across deep loops
        │
        └── rounds += 1 → GOTO submit

  └── 4. Synthesis fallback (S24, post-loop):
        │
        ├── [content.is_empty() && rounds > 0]?
        │     └── extract_tool_results(messages)  (concat role:"tool" entries)
        │           ├── None → no results, surface degenerate state
        │           └── Some(text) → continue
        │
        ├── build_synthesis_messages(prompt, results)
        │     → [system_directive, user_prompt, system_with_results]
        │       (NO assistant.tool_calls history, NO tools schema)
        │
        ├── submit synthesis job  (tools=None, fresh messages)
        │
        └── collect_round → text content
              ├── non-empty → replace `content`, clear `final_tool_calls`
              └── still empty → fall through to degenerate result
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
  └── return (result_text, ToolCallRecord)  ← caller does batch INSERT after all calls
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
| Convergence boundary | last round | At `round + 1 == max_rounds`, if `rounds > 0` and no text yet → (a) inject system message + (b) omit `tools` schema from the final-round submit. Ollama silently drops `tool_choice` (issue #8421/#11171), so schema-removal is the only reliable text-forcing knob. Tool results stay in messages so the model can synthesize. (S23) |
| Synthesis round | post-loop | If the loop exhausts with no text content, dispatch one extra inference call on a fresh messages array `[system_directive, user_prompt, system_with_tool_results]` — no `assistant.tool_calls` history, no `tools` schema. Qwen3-Coder mimics prior tool_call patterns from history even with no schemas (Qwen #475); the synth round removes that signal entirely. Final guarantee that an MCP-routed inference returns text. (S24) |
| First-token timeout | 240s | `FIRST_TOKEN_TIMEOUT` — covers 200K-context cold load (PR #90) |
| Stream-idle timeout | 45s | `STREAM_IDLE_TIMEOUT` — token-to-token gap on warm model |
| Round total timeout | 360s | `ROUND_TOTAL_TIMEOUT` — aligned with `INFERENCE_ROUTER_TIMEOUT` |
| Max concurrent tool calls | 8 | `buffered(8)` in execute_calls |
| Max tool result size | 32 KB | Truncated before injection |
| Max tools per request | 32 | Context window protection |
| Result cache TTL | 300s | Idempotent tool calls |

> Phased timeouts (PR #90) replace the prior single 45 s round timer. With
> `MCP_LIFECYCLE_PHASE=on`, Phase 1 (`ensure_ready`) absorbs cold-load timing
> as its own observable span (see `flows/model-lifecycle.md`); the bridge
> phased timeouts remain as defense-in-depth.

---

## Audit read-side

`batch_insert_tool_calls` writes every executed tool to `mcp_loop_tool_calls`
(CDD `inference/mcp-schema.md`). Read-side projection:

```
GET /v1/conversations/{id}/turns/{job_id}/internals
  └── conversation_handlers::get_turn_internals
        ├── load S3 ConversationRecord → compressed + vision_analysis
        └── SELECT … FROM mcp_loop_tool_calls t
              LEFT JOIN mcp_servers s ON s.id = t.server_id
              WHERE t.job_id = $1
              ORDER BY t.loop_round ASC, t.created_at ASC
            → tool_calls: [{round, server_slug, tool_name, namespaced_name,
                            args, result_text, outcome, cache_hit,
                            latency_ms, result_bytes, created_at}, …]
```

UI: `web/components/turn-internals.tsx` renders the timeline below each
assistant bubble in the test panel. Empty array when no MCP tools were
invoked. SDD: `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md`.

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
| `infrastructure/outbound/mcp/bridge.rs` | `McpBridgeAdapter` — native + forced-JSON loops |
| `infrastructure/outbound/mcp/forced_json.rs` | Forced-JSON gateway shim (schema, parser) for non-native-tool-calling models |
| `infrastructure/inbound/http/openai_handlers.rs` | Entry, `should_intercept()`, `mcp_ollama_chat()` |
| `infrastructure/inbound/http/mcp_handlers.rs` | MCP server CRUD, `discover_and_persist_tools()` |
| `infrastructure/inbound/http/key_mcp_access_handlers.rs` | ACL management REST API |
| `veronex-mcp/src/tools/` | MCP tools (get_weather, web_search) |
| `infrastructure/inbound/http/mcp_handlers.rs` | MCP server CRUD + `discover_and_persist_tools()` |
| `veronex-embed/src/` | Embedding service (multilingual-e5-large) |
