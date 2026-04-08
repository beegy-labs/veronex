# veronex-agent Scrape Cycle

> **Last Updated**: 2026-03-29

---

## Process Overview

```
veronex-agent (standalone binary, docker-compose service)
  │
  ├── env: VERONEX_API_URL, OTEL_HTTP_ENDPOINT, SCRAPE_INTERVAL_MS=60000
  │         REPLICA_COUNT=1 (fallback), HEALTH_PORT=9091, VALKEY_URL, DATABASE_URL
  │         EMBED_URL (veronex-embed service for tool embedding)
  │         Replica count: dynamic via SCARD veronex:agent:instances (REPLICA_COUNT=fallback)
  │
  ├── Background tasks on startup:
  │     ├── health HTTP server (port 9091) → /startup /ready /health
  │     └── orphan_sweeper (when VALKEY_URL + DATABASE_URL set)
  │           └── cancels stale in-flight jobs (no heartbeat renewal)
  │
  └── Main loop (SCRAPE_INTERVAL_MS):
        └── scrape_cycle()
```

---

## `scrape_cycle()` — Per Interval

```
scrape_cycle()
  │
  ├── 1. MCP health checks (when VALKEY_URL set)
  │     │
  │     ├── GET {veronex}/v1/mcp/targets
  │     │     └── returns [{id, url}] for all enabled MCP servers
  │     │
  │     └── for each server (concurrent join_all):
  │           scraper::ping_mcp_server(url /health + JSON-RPC ping)
  │             ├── alive  → set_mcp_heartbeat(Valkey SET EX)
  │             │     key: veronex:mcp:heartbeat:{server_id}  TTL=180s
  │             │   → collect online servers for discover_and_embed
  │             └── dead   → heartbeat not renewed → server goes offline
  │
  │     mcp_discover::discover_and_embed(online_servers, EMBED_URL)
  │       └── tools/list diff → embed changed tools → Valkey vectors
  │
  ├── 2. Metrics target discovery
  │     │
  │     ├── GET {veronex}/v1/metrics/targets
  │     │     └── returns [{targets:[host:port], labels:{type,server_id,provider_id,...}}]
  │     │
  │     └── shard filter: shard_key % replicas == ordinal
  │           replicas = SCARD veronex:agent:instances (dynamic, per cycle)
  │           → each agent pod owns a subset of targets (no coordination needed)
  │
  └── 3. Scrape targets (concurrent, semaphore MAX=32)
        │
        ├── type=server → scrape_node_exporter(url)
        │     └── Prometheus text → Vec<Gauge>
        │           → OTLP push to otel-collector → ClickHouse metrics
        │
        └── type=ollama → scrape_ollama_raw(url + /api/ps)
              ├── Vec<Gauge> → OTLP push
              ├── heartbeat: set_online(Valkey, provider_id, TTL=180s)
              │     key: veronex:heartbeat:{provider_id}
              └── capacity_push::push()
                    └── pushes loaded model list + VRAM state to veronex
                          for dispatcher VRAM pool warm-up
```

---

## Sharding (Dynamic Multi-Replica)

```
Per scrape cycle:
  1. SADD veronex:agent:instances {hostname}       ← self-register
  2. SET  veronex:agent:hb:{hostname} "1" EX 180   ← heartbeat
  3. replicas = SCARD veronex:agent:instances       ← dynamic count
  4. owns(shard_key, ordinal, replicas) → filter targets

On SIGTERM (graceful shutdown):
  SREM veronex:agent:instances {hostname}
  DEL  veronex:agent:hb:{hostname}

Fallback: REPLICA_COUNT env (used when Valkey unavailable)
```

KEDA scales the StatefulSet → new pod auto-registers → all pods see updated SCARD on next cycle → automatic re-sharding without coordination.

```
owns(key, ordinal, replicas):
  hash(key) % replicas == ordinal

→ each provider scraped by exactly 1 agent replica
```

---

## MCP Tool Discovery + Embedding

```
Two discovery paths (both write to same DB + Valkey):

1. Registration-time (veronex, immediate):
   POST /v1/mcp/servers → discover_and_persist_tools()
     └── tools/list → mcp_server_tools + mcp_servers.tools_summary
         └── tool_cache.cache_fetched_tools() (Valkey warm)

2. Periodic (veronex-agent, per scrape cycle):
   scrape_cycle() → mcp_discover::discover_and_embed()
     │
     ├── for each online MCP server:
     │     ├── tools/list → SHA-256 hash comparison with previous
     │     ├── changed? → embed via veronex-embed → Valkey vectors
     │     │     POST {EMBED_URL}/embed/batch
     │     │     → HSET mcp:vec:{server_id}:{tool_name} vector/text/spec
     │     ├── added tools   → embed + store
     │     ├── deleted tools  → remove vectors
     │     └── unchanged     → skip
     │
     └── stores mcp:tools_hash:{server_id} for next diff
```

---

## Heartbeat Key Schema

```
Provider liveness:   veronex:provider:hb:{provider_id}  TTL=180s
MCP server liveness: veronex:mcp:heartbeat:{server_id}  TTL=180s
Agent self-register: veronex:agent:instances             SET (SADD/SREM)
Agent heartbeat:     veronex:agent:hb:{hostname}         TTL=180s

Survives: 2 missed scrape cycles (default 60s interval × 3 TTL multiplier)
On expiry: provider/server/agent marked offline
```

---

## Files

| File | Purpose |
|------|---------|
| `crates/veronex-agent/src/main.rs` | Main loop, scrape_cycle, MCP health + discover |
| `crates/veronex-agent/src/scraper.rs` | node-exporter scrape, Ollama scrape, ping_mcp |
| `crates/veronex-agent/src/mcp_discover.rs` | MCP tool discovery + embedding pipeline |
| `crates/veronex-agent/src/heartbeat.rs` | Valkey heartbeat SET EX |
| `crates/veronex-agent/src/capacity_push.rs` | VRAM state push to veronex |
| `crates/veronex-agent/src/shard.rs` | Replica sharding logic |
| `crates/veronex-agent/src/orphan_sweeper.rs` | Stale job cleanup |
