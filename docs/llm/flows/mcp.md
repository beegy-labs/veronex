# MCP Agentic Loop Flow

> **Last Updated**: 2026-05-04 (constrained-decoding unification)

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

## `run_loop()` — Unified Constrained-Decoding Loop

The legacy native path (model self-decides via OpenAI `tools[]`) was removed
2026-05-04. Every MCP-routed request now runs through GBNF logit masking;
the `heuristic_supports_native` model allow-list and the S23/S24 reactive
patches it required no longer apply. SDD
`.specs/veronex/mcp-constrained-decoding-unification.md`.

```
run_loop(state, caller, model, messages, base_tools, conversation_id, stop, seed,
         response_format, frequency_penalty, presence_penalty, sse_tap_tx)
  │
  ├── 1. Per-key ACL + cap_points + top_k — parallel via tokio::join!()
  │     API key → join!(fetch_mcp_acl, fetch_mcp_cap_points, fetch_mcp_top_k)
  │               acl       → Some(HashSet<server_id>)  (empty = deny all)
  │               cap_points → 0 = MCP disabled → return None
  │                            N = max rounds (min of N, MAX_ROUNDS)
  │               top_k     → Vespa ANN limit override (None = global default)
  │     JWT     → None / MAX_ROUNDS / None (bypass all)
  │
  ├── 2. Context-budget gate (S17 Tier C/D)
  │     prune accumulated messages to fit smallest configured_ctx
  │
  ├── 3. Build tool list (Vespa ANN top-K or get_all fallback)
  │     merge base_tools + MCP tools (cap: MAX_TOOLS_PER_REQUEST=32)
  │     all_tools.is_empty() → return None
  │
  └── 4. Delegate → run_loop_forced_json(...):
        │
        ├── insert system prompt at messages[0]
        │     (build_forced_json_system_prompt — tool catalogue + tool-first directive)
        │
        └── for round in 0..max_rounds:
              │
              ├── allow_final = allow_final_for_round(prior_tool_calls.len())
              │     └── round 0 → false → schema has tool branches ONLY
              │     └── after ≥1 tool result → true → schema also has
              │           {action:"final", answer:string} and
              │           {action:"refuse", reason:string(minLength=8)}
              │
              ├── schema = build_forced_json_schema(all_tools, allow_final)
              │     └── oneOf [tool_branches..., final?, refuse?]
              │
              ├── submit job:
              │     tools = None
              │     response_format = {type:"json_schema",
              │                        json_schema:{schema}}
              │     → Ollama adapter forwards as `format` → llama.cpp GBNF mask
              │
              ├── collect_round(job_id) → RoundResult { content, tokens, ... }
              │     content is grammar-bound JSON
              │
              ├── parse_forced_action(content):
              │     ├── Tool { name, args }    → execute_calls (buffered MAX=8)
              │     │                             append assistant action +
              │     │                             user observation to messages
              │     │                             continue loop
              │     ├── Final { answer }       → content = answer; break
              │     └── Refuse { reason }      → content = REFUSAL_PREFIX + reason
              │                                  break
              │
              └── loop detection: (name, args_hash) × LOOP_DETECT_THRESHOLD=3
                    → break early with synthetic loop-detected content
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
| Round-0 tool enforcement | schema | `allow_final_for_round(0) == false` → schema has tool branches only. Logit-mask prevents pre-tool disclaimer prose. Replaces legacy reactive S23 convergence boundary. |
| Final-answer constraint | schema | `final.answer` generated under GBNF — model cannot escape JSON envelope; disclaimer-after-tools is auditable (parses back as `Final { answer }` with the failed citation visible alongside `tool_calls[]`). Replaces legacy S24 synthesis fallback. |
| Structured refusal exit | schema | `{action:"refuse", reason:string(minLength=8)}` branch (gated by `allow_final`) — the only structurally-valid non-tool exit when no tool fits. UI sentinel: `forced_json::REFUSAL_PREFIX = "REFUSED: "`. |
| First-token timeout | `MCP_TOKEN_FIRST_TIMEOUT=300s` | After Phase 1 lifecycle completes |
| Stream-idle timeout | `MCP_STREAM_IDLE_TIMEOUT=45s` | Token-to-token gap on warm model |
| Round total timeout | `MCP_ROUND_TOTAL_TIMEOUT=1500s` | Strictly under Cilium HTTPRoute 1800s |
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
invoked. SDD: `.specs/veronex/history/mcp-tool-audit-exposure-and-loop-convergence.md` (superseded for loop-convergence by `.specs/veronex/mcp-constrained-decoding-unification.md`; audit half remains canonical).

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
