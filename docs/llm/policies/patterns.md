# Code Patterns: Rust — Index

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Rust Edition 2024 · tokio 1.47 LTS (pin) · Axum 0.8 · sqlx 0.8 · OpenTelemetry 0.31
> Frontend patterns → `policies/patterns-frontend.md`
> Rust test patterns → `policies/testing-strategy-rust.md`
> Full rule text lives in `patterns/{domain}.md`; this file is an index + Quarterly Audit.

## Index by Domain

| Domain | File | Covers |
|---|---|---|
| HTTP Handlers & Errors | [`patterns/http.md`](patterns/http.md) | Axum handlers, `AppError` + Problem Details, input validation, SSE, image inference, cookies |
| Tower Middleware & Routing | [`patterns/middleware.md`](patterns/middleware.md) | Tower layer order, `TimeoutLayer`, per-key concurrency, port+adapter wiring |
| sqlx & Database Patterns | [`patterns/persistence.md`](patterns/persistence.md) | `sqlx`, pagination, pool config, batch writes, domain enums, SQL constants |
| Async, Concurrency & Performance | [`patterns/async.md`](patterns/async.md) | `tokio` LTS, `JoinSet`, mutex rules, DashMap, VramPool CAS, performance |
| Tracing, Metrics & Auditing | [`patterns/observability.md`](patterns/observability.md) | `tracing` + OpenTelemetry (0.31), audit trail, Valkey error observability |
| Valkey (Redis-compatible) Patterns | [`patterns/valkey.md`](patterns/valkey.md) | Key registry, Lua atomic evals, job counters |
| Scheduling, Liveness & Scale | [`patterns/scheduling.md`](patterns/scheduling.md) | Timeout/TTL constants, domain services, orphan sweeper, provider liveness, scale guards |
| Security Primitives | [`patterns/security.md`](patterns/security.md) | `RequirePermission` macro, provider URL SSRF validation |
| MCP Integration | [`patterns/mcp.md`](patterns/mcp.md) | MCP integration patterns |
| Build, Test & Utility Conventions | [`patterns/tooling.md`](patterns/tooling.md) | Docker build cache, test code conventions, UTF-8 safe truncation |

## Section Location (cross-ref resolution)

When a rule references `patterns.md § X`, use this table to locate the full text.

| § Section | File |
|---|---|
| Axum 0.8 Handler Signature | `patterns/http.md` |
| AppError (thiserror v2) + Problem Details (RFC 9457) | `patterns/http.md` |
| sqlx -- Compile-Time SQL | `patterns/persistence.md` |
| Pagination Pattern | `patterns/persistence.md` |
| async-trait (Required) | `patterns/async.md` |
| tracing + OpenTelemetry | `patterns/observability.md` |
| Mutex Rules — `std` vs `tokio` | `patterns/async.md` |
| DashMap (not `Mutex<HashMap>`) | `patterns/async.md` |
| Valkey Key Registry | `patterns/valkey.md` |
| Valkey Error Observability | `patterns/observability.md` |
| Valkey Lua Eval | `patterns/valkey.md` |
| Performance Patterns | `patterns/async.md` |
| Domain Services | `patterns/scheduling.md` |
| VramPool CAS Safety | `patterns/async.md` |
| Timeout & TTL Constants | `patterns/scheduling.md` |
| Tower Layer Order — `ServiceBuilder` SSOT | `patterns/middleware.md` |
| TimeoutLayer — `tower_http` over `tower` | `patterns/middleware.md` |
| Per-Key Concurrent Connection Limit (LLM Gateway) | `patterns/middleware.md` |
| tokio — LTS Pin + Manual Runtime Builder | `patterns/async.md` |
| Background Tasks -- JoinSet + CancellationToken | `patterns/async.md` |
| Pool Configuration | `patterns/persistence.md` |
| Adding a New Port + Adapter | `patterns/middleware.md` |
| RequirePermission Macro | `patterns/security.md` |
| Audit Trail (`emit_audit`) | `patterns/observability.md` |
| Batch DB Writes (N+1 Prevention) | `patterns/persistence.md` |
| Domain Enum Patterns | `patterns/persistence.md` |
| SQL Column Constants | `patterns/persistence.md` |
| Shared Persistence Helpers | `patterns/persistence.md` |
| SQL Fragment Constants | `patterns/persistence.md` |
| SQL Multi-Value Filters | `patterns/persistence.md` |
| SQL Interval Parameterization | `patterns/persistence.md` |
| Image Inference — 3-Endpoint Support | `patterns/http.md` |
| Input Validation | `patterns/http.md` |
| Shared Handler Helpers | `patterns/http.md` |
| Cookie TTL Constants | `patterns/http.md` |
| Provider URL Validation (SSRF) | `patterns/security.md` |
| Provider Liveness — Push Model (Heartbeat) | `patterns/scheduling.md` |
| Job Counters — Valkey INCR/DECR | `patterns/valkey.md` |
| Scale Guards — 10K+ Provider Patterns | `patterns/scheduling.md` |
| SSE Error Sanitization | `patterns/http.md` |
| Orphan Sweeper — Agent-Side Crash Recovery | `patterns/scheduling.md` |
| Cross-Module Error Sentinel Constants | `patterns/middleware.md` |
| Docker Build Cache — `sharing=locked` | `patterns/tooling.md` |
| Test Code Conventions | `patterns/tooling.md` |
| UTF-8 Safe Truncation | `patterns/tooling.md` |
| MCP Integration Patterns | `patterns/mcp.md` |
| Lifecycle Port Pattern (Phase 1 ↔ Phase 2 SoD) | `patterns/async.md` |

## Quarterly Audit Commands

Run these greps to surface violations. Expected results noted per check.

```bash
# P1 — SSRF: validate_provider_url called for URL inputs
grep -rn "url.*String\|String.*url" crates/veronex/src/infrastructure/inbound/ --include="*.rs" -l
# → check each file for validate_provider_url call

# P1 — SQL Interval: must use make_interval(), never format!()
grep -rn "INTERVAL.*format!\|format!.*INTERVAL" crates/ --include="*.rs"
# → expected: 0 results

# P1 — UTF-8 truncation: must use is_char_boundary() or truncate_at_char_boundary()
grep -rn "\.truncate(" crates/ --include="*.rs"
# → all calls must be preceded by is_char_boundary() scan or delegated to truncate_at_char_boundary()

# P1 — Valkey: no silent error discard on I/O
grep -rn "\.await\.unwrap_or\b\|let _.*\.await\|\.await\.ok()" crates/veronex/src/infrastructure/inbound/ --include="*.rs"
# → each result must be checked: Valkey I/O must log at tracing::warn! on error

# P2 — Valkey key hardcoding: every veronex:* string lives in either
# domain/constants.rs (canonical SSOT) or valkey_keys.rs (pk-aware shims).
grep -rn '"veronex:' crates/veronex/src/ | grep -v "valkey_keys.rs\|domain/constants.rs"
# → expected: 0 results outside test code

# P2 — Magic Duration: all timeouts via named const
grep -rn "Duration::from_secs([0-9]" crates/ --include="*.rs" | grep -v "const "
# → expected: 0 results

# P2 — O(N) DB scan: COUNT(*) in dashboard hot paths
grep -rn "COUNT(\*)" crates/veronex/src/ --include="*.rs"
# → dashboard_queries.rs must use pg_class.reltuples instead

# P2 — Unbounded SELECT: all fetch_all must have LIMIT (search persistence layer)
grep -rn "fetch_all" crates/veronex/src/infrastructure/outbound/persistence/ --include="*.rs" -B5 | grep -v "LIMIT\|ANY\|UNNEST\|--"
# → expected: 0 results — every fetch_all must have a LIMIT clause above it

# P2 — Missing emit_audit: all POST/PATCH/DELETE handlers must call emit_audit()
grep -rn "pub async fn " crates/veronex/src/infrastructure/inbound/http/ --include="*handlers*.rs" -A30 \
  | grep -B20 "\.execute\|\.insert\|\.update\|\.delete\|\.upsert" \
  | grep -v "emit_audit"
# → manual check: each mutating handler must have emit_audit after the DB write

# P2 — N+1: fetch_all/fetch_one inside for loop
grep -rn "for.*in.*{" crates/veronex/src/ --include="*.rs" -A6 | grep "fetch_all\|fetch_one\|\.execute("
# → expected: 0 results (use UNNEST or ANY($1) instead)

# P2 — Docker cache: sharing=locked on all cache mounts
grep -rn "mount=type=cache" Dockerfile* **/Dockerfile* 2>/dev/null
# → all --mount=type=cache entries must have sharing=locked

# P3 — Cross-module error sentinel: no duplicated string literals
grep -rn '"session expired"' crates/ --include="*.rs"
# → expected: 1 definition (pub(crate) const), matched via .contains() elsewhere
```
