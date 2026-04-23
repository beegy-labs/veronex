# Code Patterns: Rust — MCP Integration

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## MCP Integration Patterns

### Two-Level Tool Cache (L1 DashMap + L2 Valkey)

```
L1: DashMap<Uuid, CachedTools>  TTL 30s  — per-replica in-process
L2: Valkey SET                  TTL 35s  — cross-replica shared
Lock: Valkey SET NX             TTL 33s  — prevents thundering herd
```

Refresh sequence:
1. Check L1 TTL — if valid, return immediately (zero network)
2. Attempt `SET NX lock` — only one replica fetches at a time
3. Fetch from MCP server via `McpSessionManager`
4. Write to L1 + L2 atomically
5. Release lock

```rust
// SET NX prevents multiple replicas hitting the MCP server simultaneously
let locked: bool = conn.set(&lock_key, "1", Some(Expiration::EX(LOCK_TTL_SECS)), Some(SetOptions::NX), false).await.unwrap_or(false);
if !locked { return; }
```

### MCP Valkey Key Convention (Cross-Crate)

`veronex-mcp` defines its own key strings locally (cross-crate OK, unlike `veronex` which must use `valkey_keys.rs`).
All `veronex-mcp` keys use the `veronex:mcp:` namespace:

| Key | TTL | Purpose |
|-----|-----|---------|
| `veronex:mcp:tools:{server_id}` | 35s | L2 tool list |
| `veronex:mcp:tools:lock:{server_id}` | 33s | Refresh NX lock |
| `veronex:mcp:heartbeat:{server_id}` | set by agent | Server liveness |
| `veronex:mcp:result:{tool}:{args_hash}` | 300s | Result cache |

Rule: cross-crate local key definitions are allowed, but must be guarded with format tests.

### Input Size Guards (OOM/DoS Prevention)

Every entry point that accepts external data must have `MAX_*` constants bounding input size.

Current MCP guards:

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `MAX_TOOLS_PER_SERVER` | 1,024 | `client.rs` | tools/list response |
| `MAX_TOOL_DESCRIPTION_BYTES` | 4,096 | `client.rs` | Tool description field |
| `MAX_TOOL_SCHEMA_BYTES` | 16,384 | `client.rs` | inputSchema serialized size |
| `MAX_TOOL_RESULT_BYTES` | 32,768 | `bridge.rs` | LLM injection size |
| `MAX_ARGS_FOR_HASH_BYTES` | 4,096 | `bridge.rs` | Loop-detect hashing input |
| `MAX_CANONICAL_DEPTH` | 16 | `result_cache.rs` | JSON recursion depth |
| `MAX_TOOLS_PER_REQUEST` | 32 | `bridge.rs` | Context window protection |

Rule: always pair a `MAX_*` const with a test verifying the boundary does not panic.

### Agentic Loop Duplicate Detection

Prevents infinite loops where the model repeatedly calls the same tool with identical arguments.

```rust
// (tool_name, args_hash) → call count
let mut call_sig_counts: HashMap<(String, String), u8> = HashMap::new();
// ...
let args_hash = quick_args_hash(args_str);
let count = call_sig_counts.entry((name.clone(), args_hash)).or_insert(0);
*count += 1;
if *count >= LOOP_DETECT_THRESHOLD { break; }
```

Bounds: `MAX_ROUNDS(5) × MAX_TOOLS(32) = 160` — the HashMap is fully bounded, not unbounded.

### Canonical JSON for Cache Keys

Args hash for result cache must be order-independent (same args regardless of key ordering).

```rust
fn canonical_json(v: &serde_json::Value, depth: u8) -> String {
    if depth >= MAX_CANONICAL_DEPTH { return "\"...\"".to_owned(); }  // stack overflow guard
    match v {
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<_> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);  // deterministic key order
            // ...
        }
    }
}
// key: SHA-256(tool_name + ":" + canonical_json(args)), first 8 bytes hex-encoded
```

---

