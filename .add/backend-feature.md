# Backend Feature Addition

> ADD Execution — New Rust Handler / Domain / Adapter | **Last Updated**: 2026-04-22

## Trigger

New Rust handler, domain service, repository, or outbound adapter.

## Read Before Execution

| Doc | When |
|-----|------|
| `docs/llm/policies/architecture.md` | Always — layer boundaries |
| `docs/llm/policies/patterns.md` | Always — Rust rule registry |
| `docs/llm/policies/testing-strategy.md § Rust Testing Trophy` | Always |
| `docs/llm/infra/crate-structure.md` | New crate file |
| `docs/llm/auth/security.md` | Auth / API key / RBAC |
| `docs/llm/flows/{subsystem}.md` | Control-flow change |

## Steps

| # | Action |
|---|--------|
| 1 | Classify — Domain / Application (use case) / Inbound HTTP / Outbound adapter |
| 2 | Domain: types + pure fns in `domain/` (no tokio/sqlx/reqwest) |
| 3 | Application: port trait in `application/ports/`, use case in `application/use_cases/` |
| 4 | Inbound: Axum handler in `infrastructure/inbound/http/` per `patterns/http.md § Axum 0.8 Handler Signature` |
| 5 | Outbound: adapter in `infrastructure/outbound/` implementing the port; `anyhow::Result` internal |
| 6 | Register port/adapter per `patterns/middleware.md § Adding a New Port + Adapter` |
| 7 | Tower layers via `ServiceBuilder` per `patterns/middleware.md § Tower Layer Order` |
| 8 | Add `utoipa` annotations for new HTTP routes |
| 9 | `cargo check --workspace` + `cargo clippy --workspace -- -D warnings` |
| 10 | Tests per `.add/backend-test.md` — Unit + Handler mandatory |
| 11 | `cargo nextest run --workspace` |
| 12 | Review via `.add/backend-review.md` |

## Rules

| Rule | Detail |
|------|--------|
| Layer direction | Domain → Application → Infrastructure. Never import upward |
| Async trait | `#[async_trait]` on ports (→ `patterns/async.md § async-trait`) |
| Errors | Domain `thiserror`; Infra `anyhow::Result` internal; HTTP edge → `AppError` |
| Problem Details | 4xx/5xx → `application/problem+json` (RFC 9457) |
| SQL | `sqlx::query!` / `query_as!` only — compile-time checked |
| Batch writes | UNNEST for batch INSERT (→ `patterns/persistence.md § Batch DB Writes`) |
| Timeout | Non-streaming routes have `TimeoutLayer`; SSE in separate router |
| Valkey | Single Lua eval for multi-op atomicity |
| Observability | `#[instrument]` on every handler; span `{METHOD} {route}` |
| OTel | All `opentelemetry*` crates same minor (current 0.31) |
| tokio | `~1.47` LTS; custom Builder in `main.rs`; no `#[tokio::main]` |
| Mutex | `std::sync::Mutex` default; `tokio::sync::Mutex` only across `.await` |
| Scale | 10K providers / 1M TPS — no O(N) DB scans, no per-request allocations in hot paths |

## Checklist

- [ ] Domain types (no I/O imports)
- [ ] Port trait + use case
- [ ] Handler with `#[instrument]`
- [ ] Adapter implementing the port
- [ ] `utoipa` annotations
- [ ] Tower layers in mandated order
- [ ] Tests per `backend-test.md`
- [ ] `cargo check` + `clippy -D warnings` + `nextest run` all pass
- [ ] Review passed
