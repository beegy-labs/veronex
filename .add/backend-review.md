# Backend Review

> ADD Execution — Rust Optimization & Policy Enforcement | **Last Updated**: 2026-04-22

## Trigger

Rust code review, architecture audit, performance/security review, or refactor touching `crates/**/*.rs`.

## Read Before Execution

| Doc | When |
|-----|------|
| `docs/llm/policies/architecture.md` | Always — layer boundaries |
| `docs/llm/policies/patterns.md` | Always — full rule registry |
| `docs/llm/policies/testing-strategy.md § Rust Testing Trophy` | Always |
| `docs/llm/auth/security.md` | Auth / API key / RBAC |
| `docs/llm/infra/` | Observability changes |
| `docs/llm/flows/{subsystem}.md` | Algorithm changes |

## Non-Goals (reject on sight)

- Files in `domain/` importing `tokio` / `sqlx` / `reqwest` / `infrastructure::*`
- Upward imports (infrastructure → application → domain only)
- `anyhow` in `domain/` or `application/` public APIs
- `#[tokio::main]` in production binaries
- `axum::middleware::from_fn` when a `tower_http` layer exists
- `pub(crate)` opened solely for tests
- Generic `services/` / `helpers/` / `utils/` directories

## Steps

| # | Action |
|---|--------|
| 1 | Diff: `git diff develop..HEAD` or user-specified files |
| 2 | Run 3 parallel agents (Reuse · Quality · Efficiency) with the full diff |
| 3 | Aggregate findings; discard false positives with reason |
| 4 | Fix P0 → P1 → P2 |
| 5 | `cargo check --workspace` + `cargo clippy --workspace -- -D warnings` |
| 6 | `cargo nextest run --workspace` — zero failures |
| 7 | Repeat 2–6 until no violations remain |
| 8 | CDD feedback — `.add/cdd-feedback.md` on new pattern |

## Agent Scope

**Reuse agent** — existing abstractions over reinvention:
- `AppError` for HTTP, never raw `StatusCode` returns (→ `patterns/http.md § AppError`)
- `ProblemDetails` body for 4xx/5xx (→ `patterns/http.md § AppError (thiserror v2) + Problem Details (RFC 9457)`)
- `#[async_trait]` on ports (→ `patterns/async.md § async-trait`)
- `sqlx::query!` / `query_as!` — never raw `&str` SQL (→ `patterns/persistence.md § sqlx`)
- Batch UNNEST writes (→ `patterns/persistence.md § Batch DB Writes`)
- Valkey Lua for multi-op atomicity (→ `patterns/valkey.md § Valkey Lua Eval`)
- `tower_http::TimeoutLayer` — never `tower::timeout` (→ `patterns/middleware.md § TimeoutLayer`)
- `JoinSet` + `CancellationToken` — never detached spawn (→ `patterns/async.md § Background Tasks`)
- Domain enums shared across modules (→ `patterns/persistence.md § Domain Enum Patterns`)
- Persistence helpers (`parse_db_enum`, `SOFT_DELETE`) (→ `patterns/persistence.md § Shared Persistence Helpers`)
- `RequirePermission` macro (→ `patterns/security.md § RequirePermission Macro`)
- `emit_audit` helper (→ `patterns/observability.md § Audit Trail`)
- Timeout/TTL constants SSOT — never inline `Duration::from_secs(30)` (→ `patterns/scheduling.md § Timeout patterns/scheduling.md § Timeout & TTL Constants TTL Constants`)

**Quality agent** — correctness and compliance:
- Layer direction enforced (→ `architecture.md`)
- Handler signature + `#[instrument]` with span name `{METHOD} {route}` (→ `patterns/http.md § Axum 0.8 Handler Signature`, `§ tracing + OpenTelemetry`)
- SSE / streaming routes in a separate router without `TimeoutLayer`
- No raw SQL strings outside `persistence/`; no `SELECT *`; every `fetch_all` has `LIMIT`
- DB pool per `patterns/persistence.md § Pool Configuration`
- `std::sync::Mutex` default; `tokio::sync::Mutex` only across `.await` (→ `patterns/async.md § Mutex Rules`)
- `DashMap` `Ref`/`RefMut` dropped before `.await`
- `emit_audit` on state-changing handlers; `RequirePermission` on protected routes
- Input validated at handler boundary (size / charset / range)
- Provider URLs validated against SSRF allowlist
- Tests behavior-driven; no `pub(crate)` backdoors; no mock-call-count as primary (→ `testing-strategy.md § Behavior-Driven Rust Tests`)
- Handler tests use `tower::ServiceExt::oneshot` — not `reqwest` (→ `testing-strategy.md § Axum Handler Test Pattern`)
- Integration uses `testcontainers-rs`; outbound HTTP uses `wiremock`
- OpenTelemetry crates pinned to the same minor (current 0.31) — grep `^opentelemetry` Cargo.toml files, assert unique minor (→ `patterns/observability.md § tracing + OpenTelemetry`)
- tokio pinned to `~1.47` LTS (→ `patterns/async.md § tokio — LTS Pin`)

**Efficiency agent** — performance and scale:
- No O(N) per-provider DB scans — UNNEST or JOIN
- No sequential `.await` in hot paths — `tokio::join!` or `JoinSet`
- Counter updates via `INCR/DECR`, not GET+SET
- Lua scripts via `SCRIPT LOAD` + `EVALSHA`, not inline EVAL per call
- `&str` / `Cow<'_, str>` in hot paths, not `String` allocation
- Indexed column on new DB query paths (verify `EXPLAIN`)
- `CompressionLayer` on response-heavy routes
- Histogram for handler latency; Counter for failure classes
- Bounded `mpsc::channel(N)` — never unbounded for SSE

## Fix Iteration Policy

- One logical fix per round
- `cargo check` + `nextest run` every 3–4 rounds
- False positives count as a round (document reason)
- Stop early if no violations remain
- Parallel agents run **before** fixes begin, never interleaved
