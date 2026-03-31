# MCP Integration SDD

> **Status**: In Progress — Phase 1 complete | **Last Updated**: 2026-03-22
> **Scope**: S11 — Native MCP support via McpBridgeAdapter
> **Branch**: `feat/mcp-integration`
> **Spec Target**: MCP 2025-03-26 (Streamable HTTP)

---

## Purpose

When a user says "Tell me today's weather in Seoul":
- No manual tool selection or direct MCP calls
- LLM (Ollama) inspects the registered tool list and decides autonomously
- Calls required tools in parallel/sequential order (get_coordinates → get_weather)
- Returns only the final answer to the client

**Veronex = A server that behaves like Cursor**
Just as Cursor handles the MCP loop on the client side, Veronex handles it identically inside the server.

---

## Concrete Example

```
Client: "What's the weather in Seoul today?"

[1] Veronex — Determine MCP necessity
    Inject tool list from McpToolCache → call Ollama
    LLM decides: "Need get_coordinates, get_weather"

[2] Round 1 — tool_calls returned:
    tool_call_1: mcp_weather_get_coordinates("Seoul")
    → join_all() parallel execution (1 in this case)
    → result_cache miss → McpHttpClient.call()
    ← {lat: 37.5, lng: 126.9}

[3] Round 2 — Append results, re-request:
    tool_call_1: mcp_weather_get_weather(37.5, 126.9)
    → result_cache hit (cache_ttl_secs configured) → Valkey instant return
    ← {temp: "12°C", sky: "clear"}

[4] Round 3 — No tool_calls:
    LLM: "Today in Seoul, it's clear and 12°C."

[5] Client ← "Today in Seoul, it's clear and 12°C."
    (No visibility into intermediate steps, standard OpenAI response)
```

---

## Design Principles

1. **LLM selects tools** — No manual tool specification. If no tool_calls, MCP is never invoked
2. **Client-transparent** — Any client uses the standard OpenAI API as-is. No need to know MCP exists
3. **MCP servers run continuously** — Always-on HTTP servers. veronex-agent monitors health
4. **Global tool pool** — Tools from all registered MCP servers are merged and provided to the LLM
5. **Loop limiting via cap_points** — Prevents infinite loops. Per-key max tool_call round count
6. **Annotation-based caching** — Result caching only when `readOnlyHint: true AND idempotentHint: true`

---

## Architecture

```
Client (Cursor, Codex CLI, general apps)
  │ POST /v1/chat/completions
  │ (standard OpenAI format, no MCP config needed)
  ▼
Veronex — McpBridgeAdapter
  │
  ├── [1] Check API key mcp_access
  │         NO  → Direct to OllamaAdapter (existing path unchanged)
  │         YES ↓
  │
  ├── [2] Determine MCP necessity (delegated to LLM)
  │         McpToolCache.get_all() → inject tools → call Ollama
  │         LLM decides:
  │           No tool_calls → immediate final response → stream to client (end)
  │           tool_calls returned → proceed to [3]
  │
  └── [3] Parallel execution loop (until cap_points exhausted)
            Receive LLM tool_calls:
              ├── Loop detection: "tool:{sorted_args_hash}" identical for last 3 turns → force exit
              ├── mcp_* → buffer_unordered(8) parallel execution (per-call timeout: 30s)
              │     No Ollama ID → index-based mapping (tool_calls[i] ↔ results[i])
              │     Circuit-open server → skip that tool
              │     result_cache hit → instant return
              │     miss → McpHttpClient.call() → store in Valkey if cache-eligible
              ├── client tool → finish_reason: "tool_calls" → return to client
              └── Append result messages → cap -= 1 (only on success) → ClickHouse event → re-request LLM → repeat [2]
```

---

## MCP Protocol (2025-03-26 Streamable HTTP)

### Transport

| Aspect | 2024-11-05 (Legacy) | **2025-03-26 (Adopted)** |
|--------|---------------------|--------------------------|
| Endpoint | GET /sse + POST /messages (2) | **POST+GET /mcp (1)** |
| Session | None | `Mcp-Session-Id` header |
| Batch | Not supported | JSON-RPC batch support |
| Reconnection | Not supported | SSE id + Last-Event-ID |
| Streaming | SSE only | Per-request: JSON or SSE |

**Connection flow:**
```
Client → POST /mcp  (InitializeRequest)
         Accept: application/json, text/event-stream

Server ← 200 OK
         Mcp-Session-Id: <cryptographically_secure_uuid>
         Content-Type: application/json

Client → POST /mcp  (all subsequent requests)
         Mcp-Session-Id: <uuid>

Client → GET /mcp   (optional SSE for server push)
         Mcp-Session-Id: <uuid>

Client → DELETE /mcp (session termination)
         Mcp-Session-Id: <uuid>
```

**Error codes:**
```
400 — Missing session ID (non-initialization request)
404 — Session expired → remove Mcp-Session-Id header, POST new InitializeRequest (infinite 404 if header included)
-32602 — Unsupported protocol version or unknown tool
```

**Session expiry re-initialization sequence:**
```
404 received
  → session_manager.invalidate(server_id)
  → POST new /mcp without Mcp-Session-Id header (InitializeRequest)
  → Acquire new Mcp-Session-Id
  → Retry original request
```

### Initialize Handshake

McpClient must perform this on connection. Only `ping` is allowed before `initialize`.

```json
// 1. Client → Server
{
  "jsonrpc": "2.0", "id": 1, "method": "initialize",
  "params": {
    "protocolVersion": "2025-03-26",
    "capabilities": {
      "roots": { "listChanged": false }
      // sampling not declared — MCP server cannot reverse-call LLM (intentionally excluded in v1)
      // resources, prompts not declared — deferred to v2
    },
    "clientInfo": { "name": "veronex", "version": "0.11.0" }
  }
}

// 2. Server → Client
{
  "jsonrpc": "2.0", "id": 1,
  "result": {
    "protocolVersion": "2025-03-26",
    "capabilities": { "tools": { "listChanged": true } },
    "serverInfo": { "name": "weather-mcp", "version": "1.0.0" }
  }
}

// 3. Client → Server (no response, notification)
{ "jsonrpc": "2.0", "method": "notifications/initialized" }
```

**Sampling capability intentionally excluded:**
Reverse channel where MCP server requests LLM inference from Veronex via `sampling/createMessage`.
If `sampling` capability is not declared, the MCP server won't request it.
If declared but not implemented → MCP server hangs. Must exclude in v1.

### tools/list

```json
// Request
{ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }

// Response
{
  "jsonrpc": "2.0", "id": 2,
  "result": {
    "tools": [{
      "name": "get_weather",
      "description": "Get current weather for a location",
      "inputSchema": {
        "type": "object",
        "properties": { "lat": { "type": "number" }, "lng": { "type": "number" } },
        "required": ["lat", "lng"]
      },
      "annotations": {
        "readOnlyHint": true,      // no side effects
        "idempotentHint": true,    // same args → same result
        "destructiveHint": false,
        "openWorldHint": true
      }
    }]
  }
}
```

**Caching decision:** `readOnlyHint: true AND idempotentHint: true` → result cacheable. If either is false → always call directly.

### tools/call and Error Distinction

**Two error channels (must distinguish):**
```json
// [A] Tool execution failure (isError: true) — LLM sees the result and decides
{
  "jsonrpc": "2.0", "id": 3,
  "result": {
    "content": [{ "type": "text", "text": "API rate limit exceeded" }],
    "isError": true
  }
}

// [B] Protocol error — session destroyed, reconnection needed
{
  "jsonrpc": "2.0", "id": 3,
  "error": { "code": -32602, "message": "Unknown tool: invalid_tool" }
}
```

**Handling rules:**
- `[A] isError: true` → Forward to LLM as `tool` role message, LLM decides. **No cap deduction** (round without success)
- `[B] JSON-RPC error` → Circuit-open that MCP server, re-initialize session, exclude that server's tools

### JSON-RPC ping (liveness)

```json
// Request
{ "jsonrpc": "2.0", "id": 99, "method": "ping" }

// Response
{ "jsonrpc": "2.0", "id": 99, "result": {} }
```

Even if TCP connection is alive, the JSON-RPC stack may not respond, so ping is used.

---

## DB Schema

> Implementation file: `migrations/postgres/000011_mcp_capabilities.up.sql`

### mcp_servers

```sql
-- Actual implementation schema (migration 000011)
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
```

> slug is used for tool namespace: `mcp_{slug}_{tool_name}` (e.g., `mcp_weather_get_weather`)

### mcp_server_tools

```sql
CREATE TABLE mcp_server_tools (
    server_id       UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name       TEXT NOT NULL,
    namespaced_name TEXT NOT NULL,   -- "mcp_{slug}_{tool_name}"
    description     TEXT,
    input_schema    JSONB NOT NULL DEFAULT '{}',
    annotations     JSONB NOT NULL DEFAULT '{}',
    discovered_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, tool_name)
);
```

### mcp_key_access

```sql
-- API key → MCP server access control (default: deny)
CREATE TABLE mcp_key_access (
    api_key_id  UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    server_id   UUID    NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    is_allowed  BOOLEAN NOT NULL DEFAULT true,
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (api_key_id, server_id)
);
```

**cap_points examples:**
| Tier | cap_points | Supported chains |
|------|-----------|-----------------|
| Premium key | 5 | Stock research (3+ stages) |
| Basic key | 2 | Weather (get_coords → get_weather) |
| Inactive test | 0 | No MCP calls even with mcp_access |

### mcp_settings (Global Settings)

```sql
CREATE TABLE mcp_settings (
    id                        INT  PRIMARY KEY DEFAULT 1,  -- single row
    routing_cache_ttl_secs    INT  NOT NULL DEFAULT 3600,
    tool_schema_refresh_secs  INT  NOT NULL DEFAULT 30,
    embedding_model           TEXT NOT NULL DEFAULT 'bge-m3',
    max_tools_per_request     INT  NOT NULL DEFAULT 32,   -- context window protection
    max_routing_cache_entries INT  NOT NULL DEFAULT 200,  -- upper bound for cosine computation targets
    CHECK (id = 1)
);
```

### Migration Files

`migrations/postgres/000011_mcp_capabilities.up.sql` — Above 3 tables + GIN index on mcp_servers.name

---

## Component Details

### McpSessionManager

```
infrastructure/outbound/mcp/session_manager.rs

Role: Per-MCP-server session lifecycle management
  - Initialize handshake
  - Store and auto-inject Mcp-Session-Id header
  - Auto re-initialize on session expiry (404)
  - Independent session per replica (not shareable)

Structure:
  DashMap<McpServerId, McpSession>
  McpSession { session_id: String, client: reqwest::Client, initialized_at: Instant }
```

### McpHttpClient

```
infrastructure/outbound/mcp/client.rs

  initialize(url) → McpSession
  ping(session)   → Result<()>
  list_tools(session) → Vec<McpTool>
  call_tool(session, name, args) → McpToolResult
    - isError: true → Ok(McpToolResult { is_error: true, content })
    - JSON-RPC error → Err(McpProtocolError)
```

### McpToolCache (L1 DashMap + L2 Valkey)

```
infrastructure/outbound/mcp/tool_cache.rs

Structure:
  L1: DashMap<McpServerId, CachedTools>  (local, TTL 30s)
  L2: Valkey  key: veronex:mcp:tools:{server_id}  TTL: 35s

Read:
  DashMap hit → instant return (O(1))
  DashMap miss → Valkey → store in DashMap

Refresh (two triggers):
  [A] 30s polling:
      SET NX veronex:mcp:tools:lock:{server_id} → only the winning replica refreshes
      list_tools() call → transform → Valkey SET → DashMap update

  [B] notifications/tools/list_changed received (push-based):
      Received on SSE GET /mcp stream
      → Immediately invalidate that server's DashMap → re-fetch from Valkey/MCP on next request
      → Immediate refresh without SET NX contention (push signal = already changed)

Transform (MCP → OpenAI function format):
  tool.name         → "mcp_{server_name}_{tool_name}"
  tool.description  → function.description
  tool.inputSchema  → function.parameters
  tool.annotations  → internal metadata (for caching decisions, not exposed to LLM)

server_id reverse mapping (tool name → server_id):
  DashMap<String, McpServerId>  key: "mcp_{server_name}_{tool_name}"
  O(1) server_id lookup on tool_calls received (no DB query)
  Updated alongside tool schema refresh

Tool injection limit (context window protection):
  mcp_settings.max_tools_per_request (default 32)
  routing_cache hit → only matched tools (typically 2~5)
  miss → top 32 from all (active server priority, alphabetical name order)

SSE listener (notifications/tools/list_changed):
  On boot, maintain GET /mcp SSE stream to each MCP server (McpSseListenerTask)
  On receive → tool_cache.invalidate(server_id) immediately
  On disconnect → reconnect with Last-Event-ID, fallback to 30s polling on failure
```

### McpResultCache (Valkey)

```
infrastructure/outbound/mcp/result_cache.rs

Caching conditions:
  mcp_servers.cache_ttl_secs IS NOT NULL
  AND tool.annotations.readOnlyHint = true
  AND tool.annotations.idempotentHint = true

Cache key:
  veronex:mcp:result:{tool_name}:{args_hash}
  args_hash = SHA256(sort_keys(JSON(arguments)))[:16]  -- key normalization required

TTL:
  mcp_servers.cache_ttl_secs (in seconds)

Flow:
  hit  → instant return, no MCP server call
  miss → McpHttpClient.call_tool() → Valkey SETEX → return

Guide:
  Coordinates/place names (static)    → cache_ttl_secs: 86400  (24h)
  Weather/exchange rates (near-real-time) → cache_ttl_secs: 600   (10min)
  News/documents                      → cache_ttl_secs: 3600  (1h)
  Live stock prices/balances/orders   → cache_ttl_secs: NULL  (not cacheable)
```

### McpRoutingCache (Valkey + embedding)

```
infrastructure/outbound/mcp/routing_cache.rs

Role: Cache query pattern → tool list mapping (skip LLM tool selection round)

Implementation (Valkey lacks HNSW → lightweight cosine computation):
  Valkey HNSW is Redis 8.0+ only. Valkey (Redis fork) doesn't support it without extra modules.
  → Alternative: Store embedding vectors as ZSET in Valkey + compute cosine in Rust

  Flow:
  1. Query embedding: Ollama /api/embed (mcp_settings.embedding_model)
     → Vec<f32> generated
  2. Valkey HGETALL veronex:mcp:routes → load recent N entries (FIFO, default 200)
  3. Compute cosine_similarity vectors in Rust
     max cosine >= 0.92 → hit: return matched tool pattern
     < 0.92             → miss: inject all tools, LLM selects

Cache storage (after miss):
  ValkeyPort lacks HSET/HGETALL → implement via kv_set/kv_get + JSON serialization
  key: veronex:mcp:route:{sha256(embedding)[:16]}
  value: JSON { embedding: Vec<f32>, tools: Vec<String>, ts: i64 }
  TTL: mcp_settings.routing_cache_ttl_secs (kv_set's ttl_secs parameter)

  Index management:
  veronex:mcp:route:index → JSON Vec<String> (key list, updated via kv_set)
  On exceeding 200 entries → kv_del oldest key + update index

Effect:
  "Seoul weather" / "Busan weather" / "Tell me the weather" → cosine >= 0.92 → same tool pattern hit
  → Skip LLM tool selection round, save cap
  200 limit + Rust computation → O(200) without DB query (few ms)
```

### McpBridgeAdapter

```
infrastructure/outbound/mcp/adapter.rs

  Injection point: inside openai_handlers.rs
    Current handler streams use_case.stream_tokens() result directly to client.
    McpBridgeAdapter intercepts this stream and handles the tool_call loop within the handler.
    If all tool_calls are mcp_*, they are not exposed to client — executed directly and re-requested.

  Ollama tool_calls raw format (serde_json::Value):
    [{"type":"function","function":{"index":0,"name":"get_weather","arguments":{...}}}]
    Check mcp_* via name field, extract args from arguments field

  dispatch(request, api_key, ollama_adapter, state) -> Stream<ChatToken>:

    // [1] Check mcp_access
    cap = api_key_capabilities.get(api_key, "mcp_access") ?? return ollama.stream_chat()
    if cap.cap_points == 0: return ollama.stream_chat()

    // [2] Check routing_cache
    tools = routing_cache.get(query)
              .unwrap_or_else(|| tool_cache.get_all())

    // [3] tool_call loop
    remaining = cap.cap_points
    action_history: Vec<String> = vec![]  // for loop detection

    loop:
      response = ollama.stream_chat(messages + tools)

      if response.tool_calls.is_empty():
        stream_to_client(response.content)
        break

      // Loop detection: exit if same signature repeated 3 times
      signatures = response.tool_calls.map(|tc| format!("{}:{}", tc.name, sorted_args_hash(tc.args)))
      if action_history.last_n(3).contains_all(&signatures):
        messages.push(system: "Stop calling the same tool repeatedly. Give final answer with results so far")
        stream_to_client(ollama.stream_chat(messages))
        break
      action_history.extend(signatures)

      // Parallel execution (using join_all — guarantees original index order preservation)
      // buffer_unordered returns in completion order → breaks ordering in no-Ollama-ID environments
      // join_all returns Vec in input order
      // No Ollama tool_call ID → index-based mapping (tool_calls[i] ↔ results[i])
      futs = response.tool_calls.iter().map(|tc| async move {
        if is_mcp_tool(tc.name):
          if circuit_breaker.is_open(server_of(tc.name)):
            return ToolResult::skipped()
          timeout(30s, async {
            result_cache.get(tc)
              .or_else(|| mcp_client.call_tool(tc) → cache_if_eligible)
          }).await.unwrap_or(ToolResult::timeout())
        else:
          return_to_client_as_tool_call(tc)
      })
      results = join_all(futs).await  // order preservation guaranteed

      // Circuit breaker update (join_all → same index order)
      results.iter().zip(response.tool_calls.iter())
        .for_each(|(r, tc)| circuit_breaker.record(server_of(tc.name), r))

      // Append tool results to messages in index order
      // join_all preserves order → tool_calls[i] ↔ results[i] guaranteed
      messages.append(assistant_tool_calls)
      for (tc, result) in response.tool_calls.iter().zip(results.iter()):
        messages.push(tool_result(name: tc.name, content: result))

      // cap_points deduction: only for rounds with at least 1 successful tool_call
      // Rounds with only failures (isError:true) / timeout / circuit_open / cache_hit are not deducted
      round_has_success = results.iter().any(|(_, r)| r.is_success())
      if round_has_success:
        remaining -= 1

      // analytics_repo: per-tool_call event in each round (fire-and-forget, skip if None)
      if let Some(repo) = &state.analytics_repo:
        for (tc, result) in response.tool_calls.iter().zip(results.iter()):
          repo.emit("mcp.tool_call", now(), [
            ("mcp.api_key_id",   api_key.id.to_string()),
            ("mcp.server_id",    server_of(tc.name)),
            ("mcp.tool_name",    tc.name),
            ("mcp.args_hash",    sorted_args_hash(tc.arguments)),
            ("mcp.cache_hit",    result.is_cache_hit()),
            ("mcp.success",      result.is_success()),
            ("mcp.is_error",     result.is_mcp_error()),
            ("mcp.timed_out",    result.is_timeout()),
            ("mcp.circuit_open", result.is_skipped()),
            ("mcp.latency_ms",   result.latency_ms),
            ("mcp.cap_consumed", if round_has_success { 1 } else { 0 }),
            ("mcp.cap_remaining", remaining),
          ])

      if remaining == 0:
        messages.push(system: "Do not call any more tools. Answer with the results gathered so far")
        stream_to_client(ollama.stream_chat(messages))
        break

    // Save to routing_cache if it was a miss
    routing_cache.save(query, used_tools)
```

### Parallel Tool Execution Details

**Basic parallel (v1 — no prompt engineering needed):**

LLM returns multiple tool_calls at once via `parallel_tool_calls` → execute concurrently with `buffer_unordered(N)`.

```rust
// Core pattern
let results: Vec<_> = stream::iter(tool_calls)
    .map(|tc| async move { self.execute_mcp_tool(tc).await })
    .buffer_unordered(MAX_PARALLEL_TOOLS)  // concurrency limit
    .collect()
    .await;
```

**Dependent chains (handled automatically):**

```
Example: Micron stock analysis

Round 1 — LLM returns in parallel:
  search_company("Micron")        ─┐
  get_stock_price("MU")           ─┤ → join parallel execution
  search_recent_news("Micron")    ─┘

Round 2 — Additional calls based on Round 1 results:
  search_related_stocks(round1_keywords) → single execution

Round 3 — No tool_calls → final answer
```

**Plan-then-Execute (v2+ — requires prompt engineering):**

For complex multi-step scenarios, LLM first outputs a dependency graph as JSON, Veronex parses and topologically sorts it.

```
System prompt: "First output an execution plan as JSON. Group independent tools in the same round."

LLM output:
{
  "rounds": [
    ["search_company", "get_stock_price", "search_news"],  // parallel
    ["summarize_results"]                                   // dependent
  ]
}
```

> v1 implements basic parallel (automatic). Plan-then-Execute is opt-in for v2.

---

## Cap Deduction Policy

| Scenario | Cap deducted | Reason |
|----------|-------------|--------|
| Round with successful tool_call | **-1** | Actual MCP consumption |
| Round with all `isError: true` | **0** | Server failure, not user's fault |
| Round with all timeouts | **0** | Network issue, not user's fault |
| Round with all circuit_open | **0** | Server down, not user's fault |
| Round with all cache_hits | **0** | Internal Veronex processing, no MCP call |
| Loop detection exit | **0** | Round not executed |

> If 1+ success in a round, the entire round costs cap -= 1 (not per individual tool)

---

## ClickHouse Usage Tracking

### mcp_tool_calls Table

```sql
CREATE TABLE mcp_tool_calls (
    timestamp        DateTime64(3),
    api_key_id       UUID,
    mcp_server_id    UUID,
    tool_name        String,         -- "mcp_{server}_{tool}"
    args_hash        String,         -- SHA256[:16], PII excluded
    cache_hit        Bool,
    success          Bool,
    is_error         Bool,           -- MCP isError: true
    timed_out        Bool,
    circuit_open     Bool,           -- server blocked state
    latency_ms       UInt32,         -- 0 if cache_hit
    cap_consumed     UInt8,          -- 0 or 1 (per round)
    cap_remaining    UInt8
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (api_key_id, timestamp)
```

### Example Queries

```sql
-- Daily cap usage per key
SELECT api_key_id, sum(cap_consumed) as used
FROM mcp_tool_calls
WHERE timestamp >= today()
GROUP BY api_key_id

-- Cache hit rate per tool
SELECT tool_name,
       countIf(cache_hit) / count() as hit_rate,
       avg(latency_ms) as avg_latency
FROM mcp_tool_calls
WHERE success = true
GROUP BY tool_name
ORDER BY hit_rate DESC

-- Error rate per server (circuit breaker decision support)
SELECT mcp_server_id,
       countIf(is_error or timed_out) / count() as error_rate
FROM mcp_tool_calls
WHERE timestamp >= now() - INTERVAL 5 MINUTE
GROUP BY mcp_server_id
HAVING error_rate > 0.5
```

### Delivery Method

**Uses existing `analytics_repo` pipeline** (no separate ClickHouse connection needed)

```
McpBridgeAdapter
  → state.analytics_repo.emit("mcp.tool_call", attrs)  (fire-and-forget)
  → OTel LogRecord → OTel Collector → Redpanda [otel-logs] → ClickHouse

analytics_repo: Option<Arc<dyn AnalyticsRepository>> — already exists in AppState
If None, skip (no impact on inference flow in analytics-disabled environments)
```

**Event attribute mapping:**
```rust
// OTel attribute keys
"mcp.api_key_id"     → api_key.id
"mcp.server_id"      → mcp_server_id
"mcp.tool_name"      → tool_name
"mcp.args_hash"      → args_hash
"mcp.cache_hit"      → cache_hit (bool)
"mcp.success"        → success (bool)
"mcp.is_error"       → is_mcp_error (bool)
"mcp.timed_out"      → timed_out (bool)
"mcp.circuit_open"   → circuit_open (bool)
"mcp.latency_ms"     → latency_ms (u32)
"mcp.cap_consumed"   → 0 or 1
"mcp.cap_remaining"  → remaining (u8)
```

---

## Role Separation (N-Replica Environment)

| Role | Owner | Rationale |
|------|-------|-----------|
| MCP server health check | **veronex-agent** | Sharding distributes N agents → M servers evenly, no duplication |
| Direct MCP calls on API requests | **Veronex** | Real-time inference flow, routing through agent adds latency |
| result cache / routing cache | **Veronex** (shared Valkey) | Automatic sharing across N replicas |
| Tool schema refresh | **Veronex** (SET NX) | Only one replica actually refreshes, others read from Valkey |
| MCP sessions | **Veronex** per-replica independent | Mcp-Session-Id is not shareable |

### veronex-agent MCP Health Check

```
crates/veronex-agent/src/scraper.rs

scrape_mcp_health(server: McpServer):
  // HTTP health endpoint
  GET {server.url}/health  timeout: 5s
    → 200: continue

  // JSON-RPC ping (application-level liveness)
  POST {server.url}
    { "jsonrpc": "2.0", "id": 99, "method": "ping" }
    → result: {}  → online
    → 5xx / timeout → offline

  Result:
    online  → SETEX veronex:mcp:heartbeat:{mcp_server_id} 180 "1"
    offline → TTL natural expiry → mcp_servers.status = 'offline'
              Valkey DEL veronex:mcp:tools:{mcp_server_id}
              McpToolCache DashMap.remove(mcp_server_id)

  McpBridgeAdapter lookup:
    On tool_cache.get_all(), check heartbeat key existence for online status
    EXISTS veronex:mcp:heartbeat:{id} → 0 means exclude that server's tools

Sharding:
  Add "mcp" branch to shard_key()
  → N agents distribute M MCP servers evenly (same pattern as existing Ollama server sharding)
```

---

## API

### MCP Server Management (`mcp_handlers.rs`)

```
POST   /v1/mcp/servers                → Register
GET    /v1/mcp/servers                → List (paginated, ListPageParams)
PATCH  /v1/mcp/servers/{id}          → Update
DELETE /v1/mcp/servers/{id}          → Deactivate
GET    /v1/mcp/servers/{id}/tools    → View cached tool schema (including annotations)
POST   /v1/mcp/servers/sync          → Force tool schema refresh
```

**Registration request:**
```json
// Cacheable (weather, coordinates)
{
  "name": "weather-mcp",
  "url": "http://weather-mcp:3000/mcp",
  "transport": "streamable_http",
  "cache_ttl_secs": 600
}

// Not cacheable (live stock prices)
{
  "name": "stock-mcp",
  "url": "http://stock-mcp:3000/mcp",
  "transport": "streamable_http",
  "cache_ttl_secs": null
}
```

> Even with `cache_ttl_secs` set, tools not meeting `readOnlyHint AND idempotentHint` are not cached.

### API Key Capabilities (`key_capability_handlers.rs`)

```
GET    /v1/keys/{id}/capabilities
PUT    /v1/keys/{id}/capabilities/mcp_access   → { "cap_points": 3 }
DELETE /v1/keys/{id}/capabilities/mcp_access
```

### Global MCP Settings (`/v1/mcp/settings`)

```
GET   /v1/mcp/settings
PATCH /v1/mcp/settings
```

```json
{
  "routing_cache_ttl_secs": 3600,
  "tool_schema_refresh_secs": 30,
  "embedding_model": "nomic-embed-text",
  "max_tools_per_request": 32,
  "max_routing_cache_entries": 200
}
```

- `routing_cache_ttl_secs`: DB in seconds, UI accepts hours/minutes/seconds input
- `tool_schema_refresh_secs`: DB in seconds, UI accepts hours/minutes/seconds input
- `embedding_model`: Embedding model available in Ollama
- `max_tools_per_request`: Max tools injected to LLM (context window protection)
- `max_routing_cache_entries`: Upper bound for cosine computation targets (memory/performance control)

### Metrics Target Discovery Extension

```
GET /v1/metrics/targets
  → { targets: ["{mcp_url}/health"], labels: { type: "mcp", id: "{id}" } }
```

---

## TTL Settings Summary

| Setting | Location | DB Type | UI Input | Admin Scope |
|---------|----------|---------|----------|-------------|
| Result cache TTL | `mcp_servers.cache_ttl_secs` | `INT` (seconds) | Hours/min/sec or "Not set" | Per server |
| Routing cache TTL | `mcp_settings.routing_cache_ttl_secs` | `INT` (seconds) | Hours/min/sec | Global |
| Tool schema refresh interval | `mcp_settings.tool_schema_refresh_secs` | `INT` (seconds) | Hours/min/sec | Global |

**UI input → seconds conversion:**
```
1 hour 30 min   → 5400
10 min          → 600
24 hours        → 86400
Not set         → NULL (caching disabled)
```

---

## Error Handling

| Scenario | Handling |
|----------|----------|
| `isError: true` (tool execution failure) | Forward to LLM as tool_result, LLM decides. **No cap deduction**. Same error 3 times → exit |
| JSON-RPC protocol error | Circuit-open that MCP server, re-initialize session |
| MCP session expired (404) | **Remove Mcp-Session-Id header**, POST new InitializeRequest → retry |
| MCP server offline (ping failure) | Exclude that server's tools, continue with rest |
| Circuit breaker open | 5 consecutive failures → open → exclude that server's tools until health check passes |
| Per-call timeout (30s) | Timeout message in tool_result → LLM decides |
| Loop detection (same signature 3 times) | Force exit via system prompt for final answer |
| cap_points exhausted | Force final answer via system prompt, return |
| Client disconnected | Upstream MCP calls **not auto-cancelled** (only on explicit CancelledNotification) |
| Embedding failure | Skip routing cache, fallback to full tool injection |
| Tool name collision | Forced namespace (`mcp_{server}_{tool}`) |
| notifications/tools/list_changed | Immediately invalidate that server's DashMap, re-fetch on next request |

### Circuit Breaker

```
infrastructure/outbound/mcp/circuit_breaker.rs

  Note: AppState has existing circuit_breaker: Arc<CircuitBreakerMap> (for Ollama)
  MCP uses separate type McpCircuitBreaker, AppState field: mcp_circuit_breaker

  DashMap<McpServerId, McpCircuitState>

  States: Closed → Open → HalfOpen → Closed
    Closed:   Normal operation
    Open:     5 consecutive failures → skip tools, only health check allowed
    HalfOpen: After 60s, 1 probe → success: Closed, failure: remain Open
```

---

## UI

```
/providers?s=mcp  (lab gate: mcp_integration)
  ├── MCP server list
  │     Columns: Name / URL / Status / Tool count / Result cache TTL
  │
  ├── Server register/edit form
  │     Name, URL (streamable_http endpoint)
  │     Result Cache TTL:
  │       [ ] Not set  (NULL → always call MCP directly)
  │       [●] Custom   → [Hours __] [Min __] [Sec __]
  │
  ├── Tool list view (cached schema + annotations)
  │     readOnly / idempotent badge display
  ├── Manual sync button
  │
  └── Global MCP settings
        Routing cache TTL: [Hours __] [Min __] [Sec __]
        Tool schema refresh: [Hours __] [Min __] [Sec __]
        Embedding model:     [select]

/keys → Key details → Capabilities section
  ├── mcp_access toggle
  └── cap_points input (0~10, default 1)
        0: Disabled (for testing)
        1: Simple single tool
        3: Weather/search chains
        5: Complex chains like stock analysis
```

---

## Client Configuration Example

```bash
# Cursor / Codex CLI / general apps
OPENAI_BASE_URL=http://veronex/v1
OPENAI_API_KEY=vnx_xxxx   # mcp_access capability + cap_points >= 2

# No additional MCP configuration needed
# Veronex handles everything internally
```

---

## File List

| File | Role |
|------|------|
**Domain / Application**

| File | Role |
|------|------|
| `domain/entities/mcp_server.rs` | McpServer entity (including annotations) |
| `domain/entities/api_key_capability.rs` | ApiKeyCapability entity (cap_points) |
| `domain/enums.rs` | KeyCapability enum addition |
| `application/ports/outbound/mcp_server_repository.rs` | McpServerRepository trait |
| `application/ports/outbound/mcp_settings_repository.rs` | McpSettingsRepository trait |
| `application/ports/outbound/api_key_capability_repository.rs` | ApiKeyCapabilityRepository trait |

**Infrastructure — Persistence**

| File | Role |
|------|------|
| `infrastructure/outbound/persistence/mcp_server_repository.rs` | PostgresMcpServerRepository impl |
| `infrastructure/outbound/persistence/mcp_settings_repository.rs` | PostgresMcpSettingsRepository impl |
| `infrastructure/outbound/persistence/api_key_capability_repository.rs` | PostgresApiKeyCapabilityRepository impl |

**veronex-mcp crate** (`crates/veronex-mcp/src/`) — **Phase 1 complete**

| File | Role |
|------|------|
| `session.rs` | McpSessionManager (Mcp-Session-Id, 404 re-initialization, call_tool convenience method) |
| `client.rs` | McpHttpClient (Streamable HTTP 2025-03-26, initialize/ping/list_tools/call_tool) |
| `tool_cache.rs` | McpToolCache (DashMap L1 + Valkey L2, server_id reverse mapping, limit 32, get_tool_raw/all_namespaced_names) |
| `result_cache.rs` | McpResultCache (SHA256 canonical JSON key, annotations check) |
| `circuit_breaker.rs` | McpCircuitBreaker (5 failures → open → 60s HalfOpen, sync API) |
| `types.rs` | McpTool, McpToolCall, McpToolResult, McpContent |
| `bin/weather.rs` | weather-mcp example server (open-meteo.com, get_coordinates/get_weather) |

> routing_cache (Rust cosine): Planned for Phase 2

**Infrastructure — MCP Outbound** (`crates/veronex/src/infrastructure/outbound/mcp/`) — **Phase 1 complete**

| File | Role |
|------|------|
| `bridge.rs` | McpBridgeAdapter (agentic loop max 5 rounds, join_all order preservation, loop detection, stream=true support) |

**AppState additional fields (Phase 1):**
```rust
// state.rs
pub mcp_bridge: Option<Arc<McpBridgeAdapter>>,
// None = MCP disabled (default). Replaced with Some on server registration.
```

**Infrastructure — HTTP Inbound**

| File | Role | Status |
|------|------|--------|
| `openai_handlers.rs` | Added mcp_ollama_chat branch in chat_completions | Phase 1 complete |
| `mcp_handlers.rs` | MCP server CRUD API | Phase 2 |

**Bootstrap / Background Tasks**

| File | Role | Status |
|------|------|--------|
| `crates/veronex-agent/src/scraper.rs` | ping_mcp_server(), set_mcp_heartbeat() | Phase 1 complete |
| `crates/veronex-agent/src/main.rs` | MCP_SERVERS env var, scrape_cycle integration | Phase 1 complete |
| `bootstrap/background.rs` | McpSseListenerTask (list_changed receive → L1 invalidation) | Phase 2 |

**Background Tasks detail:**
```
McpSseListenerTask: Maintain GET /mcp SSE stream to each MCP server
  → Receive notifications/tools/list_changed → tool_cache.invalidate()
  → On disconnect: reconnect with Last-Event-ID → fallback to 30s polling on failure

McpAnalyticsTask: Not needed — uses state.analytics_repo directly (fire-and-forget)
```

**Migrations**

| File | Role |
|------|------|
| `migrations/postgres/000011_mcp_capabilities.up.sql` | mcp_servers, mcp_server_tools, mcp_key_access tables + GIN index |
| `migrations/postgres/000011_mcp_capabilities.down.sql` | DROP above tables |
| `migrations/clickhouse/000003_mcp_tool_calls.up.sql` | mcp_tool_calls MergeTree + mcp_tool_calls_hourly AggregatingMergeTree + Materialized View |
| `migrations/clickhouse/000003_mcp_tool_calls.down.sql` | DROP above tables/views |

---

## Scalability (Scale-Out)

### 1M+ TPS Design Fitness

| Component | Characteristics | Verdict |
|-----------|----------------|---------|
| McpToolCache DashMap | O(1) read, replica-independent | OK |
| McpResultCache Valkey | Cluster-compatible key design | OK — zero-downtime cluster migration |
| join_all parallel execution | Only waits for MCP server responses | OK — no Veronex CPU occupation |
| McpRoutingCache cosine | 200 entries x f32 = microseconds | OK |
| McpAnalyticsTask | mpsc channel buffering | OK — drop on channel full |
| Circuit Breaker | DashMap O(1) | OK |
| **Ollama inference** | GPU VRAM limited | Caution — existing capacity scheduler domain |
| **MCP servers** | External HTTP servers | Caution — outside Veronex control |

**Conclusion**: The Veronex MCP layer itself scales linearly with replica expansion. Actual bottlenecks are Ollama GPU and MCP servers.

### Valkey Cluster Key Design

```
# result cache: slot by server_id → same server results in same slot
veronex:mcp:result:{server_id}:{tool_name}:{args_hash}

# heartbeat: by server_id
veronex:mcp:heartbeat:{mcp_server_id}

# tool schema: by server_id
veronex:mcp:tools:{server_id}
veronex:mcp:tools:lock:{server_id}   TTL: tool_schema_refresh_secs + 5s

# routing cache: global hash
veronex:mcp:routes   (HSET)
```

---

## Out of Scope (v1)

- stdio MCP servers (local process spawning) — Streamable HTTP only
- 2024-11-05 legacy SSE transport — new servers use 2025-03-26 only
- MCP server authentication (OAuth 2.1) — plaintext URL only
- Gemini provider MCP support — Ollama only
- ~~Plan-then-Execute system prompt~~ — **Deprecated** (industry-wide convergence to ReAct, Veronex adopts ReAct single strategy)
- ~~MCP tool dependency graph specification~~ — **Deprecated** (Plan-Execute prerequisite, unnecessary)
- Sampling capability (MCP → Veronex LLM reverse call) — v2, intentionally undeclared in initialize
- Resources / Prompts / Completions primitives — v2
- CancelledNotification upstream propagation — v2 (v1: upstream maintained even on client disconnect)

---

## Phase 2 — Embedding-based Tool Retrieval + ReAct Enhancement

> **Status**: Design complete — awaiting implementation | **Updated**: 2026-03-28

### Background

Phase 1 uses `get_all()` to inject all online + authorized tools. Works with <10 MCP servers, but exceeds context window at 1000+ servers. Tool Retrieval layer needed.

### Design Principles

1. **ReAct single strategy** — Plan-Execute unnecessary. OpenAI/Claude/Gemini all use ReAct. Industry standard
2. **MCP is a tool supplier** — Not an API layer. Clients don't need to know about MCP
3. **Heavy work at registration, search-only at request time** — Embed at registration/update, request-time is vector search (<10ms)
4. **Fallback guaranteed** — If no embed model, maintain existing get_all()

### Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                        Client                                │
│   POST /v1/chat/completions                                  │
│   { model, messages, tools? }                                │
└────────────────────────┬─────────────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────────────┐
│                     Veronex Gateway                          │
│                                                              │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────┐ │
│  │ Auth        │  │ Rate Limiter │  │ Model Router        │ │
│  │ (API Key)   │  │ (RPM/TPM)   │  │ (Scheduler/Thermal) │ │
│  └──────┬──────┘  └──────────────┘  └─────────────────────┘ │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────────┐   │
│  │                   Tool Injection                      │   │
│  │                                                       │   │
│  │  Client tools ───────────┐                            │   │
│  │                          ├─→ Merge → send to LLM      │   │
│  │  MCP auto-inject ────────┘                            │   │
│  │    │                                                  │   │
│  │    └─ query → veronex-embed → cosine sim (Rust memory) │   │
│  │       → Top-K selection                               │   │
│  │       fallback: get_all() if no embeddings            │   │
│  └───────────────────────────────────────────────────────┘   │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────────┐   │
│  │                   ReAct Loop                          │   │
│  │                                                       │   │
│  │  for round in 0..MAX_ROUNDS {                         │   │
│  │      LLM inference (model or orchestrator_model)      │   │
│  │      │                                                │   │
│  │      ├─ No tool_calls → return final response         │   │
│  │      │                                                │   │
│  │      ├─ MCP tool_calls → parallel exec (≤8 concurrent)│   │
│  │      │   ├─ ACL check                                 │   │
│  │      │   ├─ Circuit breaker check                     │   │
│  │      │   ├─ Result cache check                        │   │
│  │      │   └─ MCP server call (JSON-RPC)                │   │
│  │      │                                                │   │
│  │      ├─ Results → messages append → next round        │   │
│  │      └─ Loop detection → force exit                   │   │
│  │  }                                                    │   │
│  └───────────────────────────────────────────────────────┘   │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────────┐   │
│  │                  Cost Recording                       │   │
│  │  mcp_loop_tool_calls (Postgres)                       │   │
│  │  mcp_tool_calls (ClickHouse — aggregation)            │   │
│  └───────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### Tool Retrieval — Embedding Pipeline

#### Registration-time Indexing

```
Admin: POST /v1/mcp/servers { name, slug, url }
         │
         ▼
  DB save (mcp_servers)
         │
         ▼
  veronex-agent: tools/list call → tool discovery
         │
         ▼
  Each tool's "{name}: {description}" text
         │
         ▼
  veronex-embed POST /embed → 1024-dim vector
         │
         ▼
  Valkey storage:
    HSET mcp:vec:{server_id}:{tool_name} vector <bytes>
    HSET mcp:vec:{server_id}:{tool_name} text   <original text>
    HSET mcp:vec:{server_id}:{tool_name} spec   <OpenAI tool JSON>
```

#### Vector Update Strategy

**3 triggers:**

| Trigger | Timing | Method |
|---------|--------|--------|
| New registration | POST /v1/mcp/servers | Immediate — agent discovers → embed → store |
| Periodic refresh | Every 10 min (configurable) | Agent re-calls tools/list on all online servers → detect diff → re-embed changed only |
| Manual refresh | POST /v1/mcp/servers/:id/refresh | Immediate — re-call tools/list for that server → full re-embed |

**Periodic refresh detail (10 min interval):**

```
veronex-agent: 10 min interval loop
  │
  ├─ Call tools/list on all online MCP servers
  │
  ├─ Diff against previous tools/list result
  │   (detect tool additions/deletions/description changes)
  │
  ├─ No changes → skip
  │
  ├─ Changes detected:
  │   ├─ Added tool → embed → store in Valkey
  │   ├─ Deleted tool → delete vector from Valkey
  │   └─ Changed tool → re-embed → overwrite in Valkey
  │
  └─ Update mcp_server_tools table (DB SSOT)
```

**Settings:**

```sql
-- mcp_settings table (existing)
tool_schema_refresh_secs  INT  NOT NULL DEFAULT 600,  -- 10 min
embedding_model           TEXT NOT NULL DEFAULT 'bge-m3',
```

#### Query-time Retrieval

```
User query: "Tell me the weather in Seoul"
         │
         ▼
  veronex-embed POST /embed → query vector (~5ms)
         │
         ▼
  Rust memory (McpToolCache): brute force cosine similarity (~1ms)
  (only online + ACL-passed servers, loaded from Valkey at startup)
         │
         ▼
  Top-K (default 10, limited by mcp_settings.max_tools_per_request)
  [get_weather: 0.94, web_search: 0.31, ...]
         │
         ▼
  Inject only Top-K tool specs to LLM → start ReAct
```

**Fallback:**
- veronex-embed service down → use existing `get_all()`
- Valkey vectors empty (cold start) → use existing `get_all()`
- Gradual improvement: search if vectors exist, full inject otherwise

### mcp_orchestrator_model Integration

```
bridge.rs run_loop:
  Model selection:
    lab_settings.mcp_orchestrator_model is set
      → Use that model for LLM calls (larger model for complex requests)
    Not set
      → Use original request model (current behavior maintained)
```

**Use cases:**
- Simple request (qwen3:8b): "Tell me the weather" → 1 round
- Complex request (qwen3-coder-next:128k): "Analyze incident + Slack report" → 3~5 rounds
- Cron batch (same large model): "Daily morning k8s status report"

### web_search Tool

```
crates/veronex-mcp/src/tools/web_search.rs

Backend selection (env):
  BRAVE_API_KEY set     → Brave Search API
  Not set               → DuckDuckGo Instant Answer

spec:
  name: "web_search"
  description: "Search the web for general information, news, articles."
  inputSchema: { query: string, count?: integer(1-10) }

Returns: [{ title, url, snippet }]
```

### Implementation Order

| Order | Task | Location |
|-------|------|----------|
| 1 | web_search tool implementation | `crates/veronex-mcp/src/tools/web_search.rs` |
| 2 | Register in veronex-mcp binary | `bin/veronex-mcp.rs` |
| 3 | veronex-embed service implementation | `crates/veronex-embed/` (Rust + ort, bge-m3) |
| 4 | Add veronex-embed to docker-compose + helm | `docker-compose.yml`, `deploy/helm/` |
| 5 | Registration-time embedding indexing | `veronex-agent` discover then veronex-embed → Valkey |
| 6 | Periodic refresh (10 min) | `veronex-agent` background loop |
| 7 | Manual refresh API | `POST /v1/mcp/servers/:id/refresh` |
| 8 | Query-time vector search | `tool_cache.get_all()` → `get_by_similarity(query)` |
| 9 | Fallback (get_all if no embeddings) | Branch in `tool_cache` |
| 10 | mcp_orchestrator_model integration | `bridge.rs` model branching |
| 11 | e2e tests | `scripts/e2e/12-mcp.sh` extension |

### veronex-embed Service

Embedding service under the veronex domain. Managed by veronex but API is public — external services like verobase can also call it.

```
veronex-embed (port 3200)
├─ POST /embed        { text: "...", model?: "bge-m3" }     → { vector: [f32], dims: 1024 }
├─ POST /embed/batch  { texts: [...], model?: "bge-m3" }   → { vectors: [[f32]], dims: 1024 }
├─ GET  /health                                             → { status: "ok" }
├─ GET  /models                                             → { models: [{ name, dims, loaded }], default }
└─ Default model: bge-m3 (1.1GB, 1024-dim, 100+ language support)

**Multi-model support (v1: single, v2: multi):**

v1 — bge-m3 single model, model parameter ignored
v2 — Enum-based supported model management:

```rust
enum EmbedModel {
    BgeM3,           // 1024-dim, multilingual
    NomicEmbedText,  // 768-dim,  English-focused
    BgeSmall,        // 384-dim,  ultra-lightweight
    // Add enum variant + ONNX file registration for future models
}

impl EmbedModel {
    fn dims(&self) -> usize {
        match self {
            Self::BgeM3 => 1024,
            Self::NomicEmbedText => 768,
            Self::BgeSmall => 384,
        }
    }
}
```

- Unsupported model request → 400 Bad Request
- GET /models to check currently loadable model list
- Response includes dims → caller knows vector dimensions
```

**Tech stack:**
- Rust + ort (ONNX Runtime) — single binary
- Default model: bge-m3 (1.1GB, 1024-dim, 100+ languages)
- CPU only (no GPU needed — inference ~3ms)
- Supported languages: Korean, English, Japanese, Spanish, Arabic, Vietnamese, German, etc.
- Multi-model support: select via `model` parameter per request

**Resources (k8s pod):**
```yaml
resources:
  requests:
    cpu: "500m"
    memory: "1.5Gi"
  limits:
    cpu: "1"
    memory: "2Gi"
```
- Cold start: <1s
- Throughput: ~200 req/s per pod
- Scale: horizontal scaling via replica increase

**Consumers:**

| Service | Purpose |
|---------|---------|
| veronex-agent | Embed descriptions on MCP tool registration/update |
| veronex | Embed query at request time → tool search |
| verobase | Log analysis, semantic search (future) |

**Vector search architecture:**
- veronex-embed: responsible for vector generation only
- Valkey: vector storage (persistence)
- Rust memory (McpToolCache): holds vector copies + brute force cosine similarity search
- 10K vectors x 1024 dims = ~40MB, full comparison <1ms

**Platform structure:**

```
veronex (LLM platform)
├─ veronex        Gateway + ReAct
├─ veronex-agent  MCP discover + monitoring
├─ veronex-mcp    MCP servers (external tools)
├─ veronex-embed  Embedding service (managed by veronex, public API)
└─ veronex-code   Code agent (future)
```

### Valkey Key Structure (Phase 2 additions)

```
# Tool vectors (stored at registration/update)
mcp:vec:{server_id}:{tool_name}  (HASH)
  vector  → <1024-dim float32 bytes>
  text    → "{tool_name}: {description}"
  spec    → <OpenAI function JSON>

# Last tools/list hash (for diff detection)
mcp:tools_hash:{server_id}  (STRING)
  → SHA-256 of sorted tools/list response
```

---

## Phase 3 — Intelligent Model Routing (Planned)

> **Status**: Planning | **Target**: When 5+ models in production

### Overview

`model: "auto"` — request analysis → optimal model selection. Replaces `mcp_orchestrator_model` field.

### Routing Dimensions

| Dimension | Signal | Example |
|-----------|--------|---------|
| Context length | Input token count | <8K → 8B, 32K+ → 30B, 128K → 79.7B |
| Task type | Semantic classification | QA → 8B, code → coder, analysis → 70B |
| Tool count | Injected MCP tools | 0 tools → small, 3+ → large |
| Image presence | `images` array length | >0 → VL preprocessing pipeline |

### VL Preprocessing Pipeline

When request contains images, route through a 3-stage pipeline instead of direct inference:

```
Request: text + N images
         │
         ▼
  [1] Context Analyzer (fast, text-only)
         Classify intent from text alone
         Determine if images need individual or batch analysis
         │
         ▼
  [2] VL Preprocessor (qwen3-vl or equivalent)
         For each image:
           → extract visual content as structured text description
           → identify objects, text (OCR), charts, diagrams
         Output: text descriptions replacing image payloads
         │
         ▼
  [3] Final LLM (auto-selected or user-specified)
         Input: original text + image descriptions (text-only)
         → Can use any model (not limited to VL-capable)
         → Larger reasoning model for complex analysis
```

**Benefits:**
- VL model handles only image understanding (what it's good at)
- Final model can be a larger text-only model (better reasoning)
- Decouples image capability from reasoning capability
- Images processed once, result reused if conversation continues

### Implementation Location

| Component | Role |
|-----------|------|
| `lab_settings.auto_routing_enabled` | Global toggle |
| `lab_settings.vl_preprocessor_model` | VL model name (default: qwen3-vl:8b) |
| `openai_handlers.rs` | Entry point — detect `model: "auto"` |
| New: `model_router.rs` | Routing logic (context/task/tool analysis) |
| New: `vl_preprocessor.rs` | Image → text conversion pipeline |

### Prerequisites

- 5+ models deployed across providers
- veronex-embed operational (for task type classification)
- VL model available (qwen3-vl:8b already on local Ollama)

### Vision MCP Tool

Single tool using qwen3-vl:8b for both OCR and visual analysis (7 languages supported):

```
vision-mcp
└─ tool: analyze_image
     model: lab_settings.vision_model (default: qwen3-vl:8b)
     LLM decides the analysis prompt based on context:
       "코드 분석해줘" → OCR-focused extraction
       "분위기 어때" → visual mood analysis
       "글자가 넘쳐" → layout/UI analysis
```

Gateway behavior when images detected + model is non-VL:
1. Store images temporarily (S3 or Valkey)
2. Inject hint: "[N images attached — use analyze_image to view]"
3. LLM uses ReAct to call analyze_image with contextual prompt

---

## Phase 4 — Conversation API (Planned)

> **Status**: Planning

### Conversation ID Format

```
UUIDv7 → base62 encode → conversation_id
128-bit → ~22 chars, URL-safe, time-ordered
Example: "7Ks9mPqR2xYvN4bW3z"
```

### Flow

```
[1] First request → generate conversation_id (base62)
    → store ConversationRecord in S3
    → return conversation_id in response

[2] Follow-up request + conversation_id
    → load ConversationRecord from S3
    → append previous messages + result
    → append new user message
    → LLM inference with full context
    → update ConversationRecord in S3
```

### Storage

- S3: full conversation (SSOT) — messages, tool_calls, results
- Postgres: conversation_id index only (for job listing/search)
- No result_text duplication in DB
