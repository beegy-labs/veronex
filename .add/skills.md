# Skills Registry

> ADD Reference | **Last Updated**: 2026-05-16

## Project Skills

| Skill | Stack | Key Files |
| ----- | ----- | --------- |
| rust-backend | Axum, sqlx, tokio, fred | `crates/veronex/` |
| rust-mcp | Axum, moka, fred, reqwest (flat module, Tool trait) | `crates/veronex-mcp/` |
| rust-agent | reqwest, OTLP, scraper, mcp_discover | `crates/veronex-agent/` |
| rust-analytics | Axum, clickhouse-rs | `crates/veronex-analytics/` |
| rust-embed | Axum, fastembed, ort (ONNX Runtime) | `crates/veronex-embed/` |
| react-frontend | Next.js 16, React 19, TanStack Query v5 | `web/` |
| migration | SQL (Postgres + ClickHouse) | `migrations/` |
| testing | cargo-nextest, vitest, bash E2E | `test/scripts/e2e/` |
| infra | Helm, Docker, K8s | `deploy/` |
| docs-policy | CDD/SDD/ADD framework | `.ai/`, `docs/llm/`, `.specs/`, `.add/` |

## Stack Changelog

| Date | Surface | Confirmed Stable Guidance |
| ----- | ----- | --------- |
| 2026-05-16 | rust-backend | Axum 0.8 state via `State`/`FromRef`; sqlx 0.8 compile-time query macros; Tokio shutdown via `CancellationToken` + `JoinSet`; keep `async-trait` on dyn ports |
| 2026-05-16 | react-frontend | Next.js 16 on Node 20.9+ baseline; React 19.2 `useEffectEvent` / transition-first responsiveness; TanStack Query v5 `queryOptions` / `mutationOptions` / `skipToken`; verodesign-only primitives/tokens |
| 2026-05-16 | testing | Vitest 4 requires Node 20+ and Vite 6+; Browser Mode remains the component-test SSOT |

## Skill Selection

Pick skill based on the files being changed. Multiple skills may apply (e.g., `rust-backend` + `migration` for a new table with API).
