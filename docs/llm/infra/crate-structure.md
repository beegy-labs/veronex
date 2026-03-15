# Crate Structure

> CDD Layer 2 | **Last Updated**: 2026-03-15

## Workspace Members

| Crate | Role | Port | Key Dependencies |
|-------|------|------|-----------------|
| veronex | Main API server + scheduler | 3000 | axum, sqlx, fred, tokio |
| veronex-agent | Metrics collector (node-exporter + Ollama scraper) | 9091 | reqwest, OTLP proto |
| veronex-analytics | ClickHouse analytics service | 3003 | axum, clickhouse-rs |

## Dependency Rules

| Rule | Detail |
|------|--------|
| No circular deps | Cargo workspace enforces |
| veronex-agent -> veronex | Not allowed (separate binary) |
| veronex-analytics -> veronex | Not allowed (separate binary) |
| Shared types | None currently; each crate defines own types |

## Build

| Command | Purpose |
|---------|---------|
| cargo check --workspace | Type check all crates |
| cargo test --workspace | Run all tests (325 total) |
| cargo clippy --workspace | Lint all crates |
