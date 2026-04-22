# Backend Feature Addition

> ADD Execution — New Rust Handler / Domain / Adapter | **Last Updated**: 2026-04-22

## Trigger

User requests a new backend handler, domain service, repository, or outbound adapter.

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Architecture rules (SSOT) | `docs/llm/policies/architecture.md` | Always — layer boundaries, crate graph |
| Rust patterns (SSOT) | `docs/llm/policies/patterns.md` | Always — 40+ rule sections |
| Testing strategy | `docs/llm/policies/testing-strategy.md § Rust Testing Trophy` | Always |
| Crate structure | `docs/llm/infra/crate-structure.md` | When adding a file to a new crate |
| Auth / security | `docs/llm/auth/security.md` | Handler touches auth / API keys / RBAC |
| Flows | `docs/llm/flows/{subsystem}.md` | Control-flow change |
| Domain doc | `docs/llm/{domain}/...` | Changing inference/providers/mcp behavior |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Classify the change — Domain? Application (use case)? Inbound HTTP? Outbound adapter? |
| 2 | **Domain**: add types + pure functions to `domain/` — zero `tokio`/`sqlx`/`reqwest` imports |
| 3 | **Application**: define port trait(s) in `application/ports/`, write use case in `application/use_cases/` |
| 4 | **Infrastructure (inbound)**: add Axum handler in `infrastructure/inbound/http/` — signature per `patterns.md § Axum 0.8 Handler Signature` |
| 5 | **Infrastructure (outbound)**: add adapter in `infrastructure/outbound/` — implements the port trait; `anyhow::Result` internal, converted to `AppError` at HTTP edge |
| 6 | Register new port + adapter per `patterns.md § Adding a New Port + Adapter` |
| 7 | Wire tower layers in `ServiceBuilder` per `patterns.md § Tower Layer Order` |
| 8 | Add OpenAPI annotations (`utoipa`) if exposing a new HTTP route |
| 9 | Run `cargo check --workspace` + `cargo clippy --workspace -- -D warnings` |
| 10 | Write tests per `.add/backend-test.md` — Unit + Handler mandatory; Integration if touching DB/queue/HTTP |
| 11 | Run `cargo nextest run --workspace` — all pass |
| 12 | Run backend review: `.add/backend-review.md` |

## Rules

| Rule | Detail |
|------|--------|
| Layer direction | Domain → Application → Infrastructure. Never import upward |
| Async trait | Application ports use `#[async_trait]` (SSOT: `patterns.md § async-trait`) |
| Error | Domain uses `thiserror`; Infrastructure uses `anyhow::Result` internally; inbound HTTP converts to `AppError` |
| Problem Details | 4xx/5xx responses return `application/problem+json` (RFC 9457) |
| No raw SQL strings | Use `sqlx::query!` / `query_as!` — compile-time checked |
| Batch writes | Use UNNEST for batch INSERT; never loop `.execute`. See `patterns.md § Batch DB Writes` |
| Timeout | Every non-streaming route has `TimeoutLayer`; SSE routes live in a separate router |
| Valkey access | Single Lua eval for multi-op atomicity (`patterns.md § Valkey Lua Eval`) |
| Observability | `#[instrument]` on every handler; span names follow `{METHOD} {route_template}` |
| OpenTelemetry | All `opentelemetry*` crates pinned to the same minor (current: 0.31). Bump all four together per `patterns.md § Workspace Version Rule` |
| tokio | `~1.47` LTS; custom runtime Builder in `main.rs`; no `#[tokio::main]` |
| Mutex choice | `std::sync::Mutex` default; `tokio::sync::Mutex` only when holding across `.await` |
| Scale | 10K providers / 1M TPS — no O(N) DB scans, no per-request allocations in hot paths |

## Output Checklist

- [ ] Domain types in `domain/` (no I/O imports)
- [ ] Port trait in `application/ports/`
- [ ] Use case in `application/use_cases/`
- [ ] Handler in `infrastructure/inbound/http/` with `#[instrument]`
- [ ] Adapter in `infrastructure/outbound/` implementing the port
- [ ] OpenAPI annotations present
- [ ] Tower layers composed via `ServiceBuilder` in the mandated order
- [ ] Tests added per `.add/backend-test.md` Layer Selection
- [ ] `cargo check --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo nextest run --workspace` passes
- [ ] Backend review passed
