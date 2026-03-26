# Skills Registry

> ADD Reference | **Last Updated**: 2026-03-25

## Project Skills

| Skill | Stack | Key Files |
|-------|-------|-----------|
| rust-backend | Axum 0.8, sqlx 0.8, tokio 1, fred 10 | `crates/veronex/` |
| rust-agent | reqwest 0.13, OTLP, scraper | `crates/veronex-agent/` |
| rust-mcp | Axum, MCP Streamable HTTP 2025-03-26 | `crates/veronex-mcp/` |
| rust-analytics | Axum, clickhouse-rs | `crates/veronex-analytics/` |
| react-frontend | Next.js 16.2, React 19.2, TanStack Query v5.95, lucide-react 1.x, Tailwind v4 | `web/` |
| migration | SQL (Postgres + ClickHouse) | `migrations/` |
| testing | cargo-nextest, vitest 4.x, Playwright 1.58, bash E2E | `scripts/e2e/`, `web/e2e/` |
| infra | Helm, Docker, K8s | `deploy/` |
| docs-policy | CDD/SDD/ADD framework | `.ai/`, `docs/llm/`, `.specs/`, `.add/` |

## Skill Selection

Pick skill based on the files being changed. Multiple skills may apply (e.g. `rust-backend` + `migration` for a new table with API, or `rust-backend` + `react-frontend` for a full-stack feature).

## Version Changelog

| Date | Change |
|------|--------|
| 2026-03-25 | `react-frontend`: Next.js 16.1→16.2, React 19.0→19.2, TanStack Query 5.0→5.95, lucide-react 0.x→1.x, Tailwind 4.0→4.2 (target); `testing`: vitest 3→4, Playwright 1.49→1.58; added `rust-mcp` skill |
