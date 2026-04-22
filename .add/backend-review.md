# Backend Review

> ADD Execution — Rust Optimization & Policy Enforcement | **Last Updated**: 2026-04-22

## Trigger

User requests a Rust code review, architecture audit, performance review, security review, or refactor alignment. Use this workflow when the change touches `crates/**/*.rs`.

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Architecture (SSOT) | `docs/llm/policies/architecture.md` | Always — layer boundaries |
| Rust patterns (SSOT) | `docs/llm/policies/patterns.md` | Always — contains all checklists |
| Testing (SSOT) | `docs/llm/policies/testing-strategy.md § Rust Testing Trophy` | Always |
| Auth / security | `docs/llm/auth/security.md` | Auth / API key / RBAC changes |
| Infra / OTel | `docs/llm/infra/` | Observability changes |
| Flows | `docs/llm/flows/{subsystem}.md` | Algorithm / control-flow changes |

> Full rule text (layers, tower order, Problem Details, OTel, tokio LTS, Valkey Lua) → `docs/llm/policies/patterns.md`

---

## Architecture Non-Goals (reject on sight)

- Rust files in `domain/` that import `tokio`, `sqlx`, `reqwest`, or any `infrastructure::*` module
- Upward imports (infrastructure → application → domain direction only)
- `anyhow` anywhere in `domain/` or `application/` public APIs
- `#[tokio::main]` in production binaries (use manual Builder)
- `axum::middleware::from_fn` when a `tower_http` layer exists
- `pub(crate)` back-doors opened solely for tests
- New directories named `services/`, `helpers/`, `utils/` without concrete domain meaning

---

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Get the diff: `git diff develop..HEAD` or read user-specified files |
| 2 | Launch 3 parallel review agents (Reuse · Quality · Efficiency) — pass full diff + agent scope below |
| 3 | Aggregate findings; discard false positives with reason |
| 4 | Fix violations P0 → P1 → P2 |
| 5 | Run `cargo check --workspace` + `cargo clippy --workspace -- -D warnings` |
| 6 | Run `cargo nextest run --workspace` — zero failures |
| 7 | If N rounds requested: repeat steps 2–6 until N rounds consumed or no violations remain |
| 8 | CDD feedback — run `.add/cdd-feedback.md` if a new pattern is confirmed (target doc table below) |

### Agent Scope

**Reuse agent** — checks that existing abstractions are used instead of reinvented:
- `AppError` for HTTP responses — never `Result<T, Box<dyn Error>>` or raw `StatusCode` returns (→ `patterns.md § AppError`)
- `ProblemDetails` for 4xx/5xx bodies — never hand-rolled JSON error bodies (→ `patterns.md § Problem Details`)
- `#[async_trait]` on application ports — never custom future box (→ `patterns.md § async-trait`)
- `sqlx::query!` / `query_as!` — never raw `&str` SQL (→ `patterns.md § sqlx`)
- Batch writes via UNNEST — never `for x { .execute(...) }` (→ `patterns.md § Batch DB Writes`)
- Valkey Lua eval for multi-op atomicity — never pipeline of GET/SET (→ `patterns.md § Valkey Lua Eval`)
- `tower_http::TimeoutLayer` — never raw `tower::timeout::TimeoutLayer` (→ `patterns.md § TimeoutLayer`)
- `JoinSet` + `CancellationToken` for background tasks — never detached `tokio::spawn` (→ `patterns.md § Background Tasks`)
- Existing domain enum in `domain/` — never redefine the same enum per module (→ `patterns.md § Domain Enum Patterns`)
- Shared persistence helpers — `parse_db_enum`, `SOFT_DELETE` (→ `patterns.md § Shared Persistence Helpers`)
- `RequirePermission` macro — never hand-rolled permission checks (→ `patterns.md § RequirePermission Macro`)
- `emit_audit` helper — never inline audit inserts (→ `patterns.md § Audit Trail`)
- Timeout/TTL constants from a SSOT module — never hardcode `Duration::from_secs(30)` inline (→ `patterns.md § Timeout & TTL Constants`)

**Quality agent** — checks correctness and pattern compliance:
- Layer direction: `domain/` imports nothing from `infrastructure/` or `application/` (→ `architecture.md`)
- Handler signature: `State`/`Path`/`Json` extractors, returns `Result<impl IntoResponse, AppError>` (→ `patterns.md § Axum 0.8 Handler Signature`)
- Handler has `#[instrument(skip(state), fields(...), name = "{METHOD} {route_template}")]` (→ `patterns.md § tracing + OpenTelemetry`)
- SSE / streaming routes merged into a separate router WITHOUT `TimeoutLayer` (→ `patterns.md § TimeoutLayer`)
- No raw SQL strings outside `persistence/` helpers (→ `patterns.md § sqlx`)
- `SELECT *` is forbidden — always explicit column list (→ `patterns.md § sqlx`)
- Every `fetch_all` has a `LIMIT` clause (→ `patterns.md § sqlx`)
- DB pool configured per `patterns.md § Pool Configuration`
- `std::sync::Mutex` default; `tokio::sync::Mutex` only when held across `.await` (→ `patterns.md § Mutex Rules`)
- `DashMap` `Ref`/`RefMut` dropped before `.await` (→ `patterns.md § DashMap`)
- `emit_audit` called on every state-changing handler (→ `patterns.md § Audit Trail`)
- `RequirePermission` macro applied on every protected route (→ `patterns.md § RequirePermission Macro`)
- Input validated at handler boundary (size caps, charset, range) (→ `patterns.md § Input Validation`)
- Provider URLs validated against SSRF allowlist (→ `patterns.md § Provider URL Validation`)
- Tests are behavior-driven: no `pub(crate)` backdoors, no mock-call-count as primary assertion (→ `testing-strategy.md § Behavior-Driven Rust Tests`)
- Handler tests use `tower::ServiceExt::oneshot` — not `reqwest` (→ `testing-strategy.md § Axum Handler Test Pattern`)
- Integration tests use `testcontainers-rs` — not mocked DB (→ `testing-strategy.md § Integration Testing`)
- Outbound HTTP tests use `wiremock` (→ `testing-strategy.md § HTTP Client Adapter Tests`)
- OpenTelemetry crates pinned to the same minor (current 0.31) — grep `^opentelemetry` Cargo.toml files and assert unique minor (→ `patterns.md § tracing + OpenTelemetry`)
- tokio pinned to `~1.47` LTS (→ `patterns.md § tokio — LTS Pin`)

**Efficiency agent** — checks performance and scale:
- No O(N) DB scans per provider/MCP — batch via UNNEST or JOIN (→ `patterns.md § Batch DB Writes`)
- No sequential `.await` in hot paths — use `tokio::join!` or `JoinSet` (→ `patterns.md § Performance Patterns`)
- Counter updates via `INCR/DECR` Valkey, not GET + SET (→ `patterns.md § Job Counters`)
- Lua scripts loaded once via `SCRIPT LOAD`, then `EVALSHA` (→ `patterns.md § Valkey Lua Eval`)
- String allocation in hot path — use `&str` or `Cow<'_, str>` where possible
- DB query uses indexed columns (verify via `EXPLAIN` for new indexed paths)
- `tower_http::CompressionLayer` on response-heavy routes
- Histogram metric for every request handler latency; Counter for every failure class
- SSE backpressure — unbounded `Sender` is banned; use bounded channels (`mpsc::channel(N)`)

**Step 8 — which doc to update:**

| What changed | Target |
|--------------|--------|
| New Rust pattern | `docs/llm/policies/patterns.md` |
| Architecture boundary / crate move | `docs/llm/policies/architecture.md` + `docs/llm/infra/crate-structure.md` |
| New testing pattern | `docs/llm/policies/testing-strategy.md` |
| Auth / security rule | `docs/llm/auth/security.md` |
| OTel / metrics | `docs/llm/infra/otel-pipeline.md` |
| Flow / algorithm change | `docs/llm/flows/{subsystem}.md` |

## Fix Iteration Policy

- Each *round* = one logical fix (a single coherent change)
- After every 3–4 rounds, run `cargo check --workspace` + `cargo nextest run --workspace`
- False positives count as a round (document why the finding was skipped)
- Stop early if no remaining violations — do not manufacture changes to hit the count
- Parallel review agents always run **before** fixes begin, not interleaved
