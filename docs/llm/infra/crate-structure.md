# Crate Structure

> CDD Layer 2 | **Last Updated**: 2026-03-28

## Workspace Members

| Crate | Role | Port | Key Dependencies |
|-------|------|------|-----------------|
| veronex | Main API server + scheduler | 3000 | axum, sqlx, fred, tokio |
| veronex-agent | Metrics collector (node-exporter + Ollama scraper) | 9091 | reqwest, OTLP proto |
| veronex-analytics | ClickHouse analytics service | 3003 | axum, clickhouse-rs |
| veronex-mcp | MCP tool server (multi-tool, single deployment) | 3100 (docker-compose) / 8080 (Helm) | axum, moka, fred, reqwest |
| veronex-embed | Embedding server | 3200 | axum, candle |
| workspace-hack | Dependency unification (hakari) | -- | -- |

## Dependency Rules

| Rule | Detail |
|------|--------|
| No circular deps | Cargo workspace enforces |
| veronex-agent -> veronex | Not allowed (separate binary) |
| veronex-analytics -> veronex | Not allowed (separate binary) |
| veronex-mcp -> veronex | Not allowed (separate binary) |
| veronex -> veronex-mcp | Allowed (client library only — `McpHttpClient`, `McpSessionManager`, etc.) |

## veronex-mcp Layout

```
crates/veronex-mcp/src/
  lib.rs          — MCP client library (used by veronex)
  client.rs       — McpHttpClient
  session.rs      — McpSessionManager
  tool_cache.rs   — McpToolCache
  result_cache.rs — McpResultCache
  circuit_breaker.rs
  types.rs
  geo/mod.rs      — Offline geocoding (GeoNames cities1000, embedded at compile time)
  tools/mod.rs    — Tool trait
  tools/weather.rs — get_weather tool (L1 Moka + L2 Valkey + singleflight)
  tools/web_search.rs — web_search tool
  bin/veronex-mcp.rs — Server binary: tool registry, JSON-RPC dispatch
```

Architecture: flat module — no hexagonal layers. Tools implement `Tool` trait; main binary registers and dispatches. To add a tool: create `tools/{name}.rs`, implement `Tool`, register in `main()`.

## Build

| Command | Purpose |
|---------|---------|
| cargo check --workspace | Type check all crates |
| cargo test --workspace | Run all tests (325 total) |
| cargo clippy --workspace | Lint all crates |
