# veronex-agent Scrape Cycle

> **Last Updated**: 2026-03-26

---

## Process Overview

```
veronex-agent (standalone binary, docker-compose service)
  │
  ├── env: VERONEX_API_URL, OTEL_HTTP_ENDPOINT, SCRAPE_INTERVAL_MS=60000
  │         REPLICA_COUNT=1, HEALTH_PORT=9091, VALKEY_URL, DATABASE_URL
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
  │           scraper::ping_mcp_server(url + /health)
  │             ├── alive  → set_mcp_heartbeat(Valkey SET EX)
  │             │     key: veronex:mcp:heartbeat:{server_id}  TTL=180s
  │             └── dead   → heartbeat not renewed → server goes offline
  │
  ├── 2. Metrics target discovery
  │     │
  │     ├── GET {veronex}/v1/metrics/targets
  │     │     └── returns [{targets:[host:port], labels:{type,server_id,provider_id,...}}]
  │     │
  │     └── shard filter: shard_key % REPLICA_COUNT == ordinal
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

## Sharding (Multi-Replica)

```
REPLICA_COUNT=N  →  agent pod ordinal derived from hostname suffix
shard_key = server_id (for server targets) or provider_id (for ollama)

owns(key, ordinal, replicas):
  hash(key) % replicas == ordinal

→ each provider scraped by exactly 1 agent replica
  no external coordination, no Valkey leader election
```

---

## MCP Tool Discovery (via session_manager, not agent)

```
NOTE: MCP tool discovery (tools/list) is performed by veronex itself,
      not by veronex-agent. The agent only handles health heartbeats.

McpSessionManager (in-process, veronex)
  │
  ├── On session connect: JSON-RPC initialize → tools/list
  │     └── tools persisted to:
  │           Postgres: mcp_server_tools (tool definitions)
  │           Valkey:   veronex:mcp:tools:{server_id} (active tool cache)
  │
  └── tool_cache.get_all() → used by bridge.run_loop() at inference time
```

---

## Heartbeat Key Schema

```
Provider liveness:   veronex:heartbeat:{provider_id}    TTL=180s
MCP server liveness: veronex:mcp:heartbeat:{server_id}  TTL=180s

Survives: 2 missed scrape cycles (default 60s interval × 3 TTL multiplier)
On expiry: provider/server marked offline by veronex
```

---

## Files

| File | Purpose |
|------|---------|
| `crates/veronex-agent/src/main.rs` | Main loop, scrape_cycle, MCP health |
| `crates/veronex-agent/src/scraper.rs` | node-exporter scrape, Ollama scrape, ping_mcp |
| `crates/veronex-agent/src/heartbeat.rs` | Valkey heartbeat SET EX |
| `crates/veronex-agent/src/capacity_push.rs` | VRAM state push to veronex |
| `crates/veronex-agent/src/shard.rs` | Replica sharding logic |
| `crates/veronex-agent/src/orphan_sweeper.rs` | Stale job cleanup |
