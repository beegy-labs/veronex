# Skills Registry

> ADD Reference | **Last Updated**: 2026-03-15

## Project Skills

| Skill | Stack | Key Files |
| ----- | ----- | --------- |
| rust-backend | Axum, sqlx, tokio, fred | `crates/veronex/` |
| rust-mcp | Axum, moka, fred, reqwest (flat module, Tool trait) | `crates/veronex-mcp/` |
| rust-agent | reqwest, OTLP, scraper | `crates/veronex-agent/` |
| rust-analytics | Axum, clickhouse-rs | `crates/veronex-analytics/` |
| react-frontend | Next.js 16, React 19, TanStack Query v5 | `web/` |
| migration | SQL (Postgres + ClickHouse) | `migrations/` |
| testing | cargo-nextest, vitest, bash E2E | `scripts/e2e/` |
| infra | Helm, Docker, K8s | `deploy/` |
| docs-policy | CDD/SDD/ADD framework | `.ai/`, `docs/llm/`, `.specs/`, `.add/` |

## Skill Selection

Pick skill based on the files being changed. Multiple skills may apply (e.g., `rust-backend` + `migration` for a new table with API).
