# Backend (Rust) Audit

> ADD Execution — Rust grep-based audit (P1/P2/P3/P4) | **Last Updated**: 2026-04-22
> Parent: `best-practices.md`. Each block notes its CDD SSOT via `# → {doc} § {section}`.


Used by `code-review.md` / `backend-review.md`. Each block includes `# → {doc} § {section}` for the CDD SSOT.

### P1 — Architecture & Correctness (always run)

```bash
# → architecture.md — domain must not import infrastructure/application concerns
grep -rn "use .*tokio\|use .*sqlx\|use .*reqwest\|use .*axum\|use crate::infrastructure\|use crate::application" crates/*/src/domain/ 2>/dev/null

# → patterns/http.md § AppError — no anyhow exposed in domain/application public APIs
grep -rn "pub.*anyhow::" crates/*/src/domain/ crates/*/src/application/ 2>/dev/null

# → patterns/persistence.md § sqlx — no raw SQL strings outside persistence helpers
grep -rn '"SELECT \|"INSERT \|"UPDATE \|"DELETE ' crates/*/src/ --include="*.rs" | grep -v persistence/ | grep -v "//\|test"

# → patterns/persistence.md § sqlx — SELECT * forbidden
grep -rn 'query!\|query_as!' crates/*/src/ --include="*.rs" | grep -i "SELECT \*"

# → patterns/middleware.md § Tower Layer Order — ServiceBuilder ordering + SetSensitiveRequestHeadersLayer before TraceLayer
grep -rn "TraceLayer\|SetSensitiveRequestHeadersLayer" crates/veronex/src/ --include="*.rs"

# → patterns/http.md § AppError + Problem Details — every 4xx/5xx body should be application/problem+json
grep -rn "Content-Type.*application/json" crates/*/src/infrastructure/inbound/http/ 2>/dev/null | grep -i "error\|4[0-9]\{2\}\|5[0-9]\{2\}"

# → patterns/async.md § Mutex Rules — no tokio::sync::Mutex without a .await inside its guarded block
grep -rn "tokio::sync::Mutex" crates/*/src/ --include="*.rs" | grep -v "//\|test"

# → patterns/middleware.md § TimeoutLayer — no raw tower::timeout
grep -rn "tower::timeout::TimeoutLayer" crates/*/src/ --include="*.rs"

# → patterns/http.md § Axum 0.8 Handler Signature — no axum::middleware::from_fn where a tower_http layer exists
grep -rn "axum::middleware::from_fn" crates/*/src/ --include="*.rs"
```

### P2 — Performance & Scale (run when touching handlers, repos, schedulers)

```bash
# → patterns/persistence.md § Batch DB Writes — no loop of execute()
grep -rB1 "\.execute(" crates/*/src/ --include="*.rs" | grep -B1 "for\|while\|\.iter()" | head

# → patterns/persistence.md § sqlx — every fetch_all must have LIMIT
grep -rn "fetch_all" crates/*/src/ --include="*.rs" | grep -v "LIMIT\|//\|test"

# → patterns/valkey.md § Valkey Lua Eval — multi-op atomicity via single EVAL
grep -rn "redis::cmd\|valkey::cmd" crates/*/src/ --include="*.rs" | grep -v "EVAL\|//"

# → patterns/async.md § Performance Patterns — no sequential awaits in hot paths
grep -rB1 "\.await;" crates/veronex/src/infrastructure/inbound/http/ --include="*.rs" | grep -B1 "\.await;" | head
```

### P3 — Observability (run when touching handlers / tasks)

```bash
# → patterns/observability.md § tracing + OpenTelemetry — every handler has #[instrument]
grep -rB2 "pub async fn.*-> Result<.*AppError>" crates/*/src/infrastructure/inbound/http/ --include="*.rs" | grep -B2 "pub async fn" | grep -v "instrument"

# → patterns/observability.md § tracing + OpenTelemetry — all opentelemetry* crates on same minor
grep -E '^opentelemetry[-_a-z]* *= *"' crates/*/Cargo.toml Cargo.toml 2>/dev/null | sort -u

# → patterns/async.md § Background Tasks — tokio::spawn without .instrument(span)
grep -rn "tokio::spawn" crates/*/src/ --include="*.rs" | grep -v "\.instrument(\|//\|test"

# → patterns/async.md § tokio — LTS Pin — no #[tokio::main] in production bins
grep -rn "#\[tokio::main\]" crates/*/src/bin/ crates/*/src/main.rs 2>/dev/null
```

### P4 — Testing (run when adding/changing tests)

```bash
# → testing-strategy.md § Axum Handler Test Pattern — no reqwest in unit/handler tests
grep -rn "reqwest::" crates/*/src/ --include="*.rs" | grep -i "test" | grep -v outbound

# → testing-strategy.md § Integration Testing — no mocked Postgres/Valkey in tests
grep -rn "MockDb\|FakePool\|sqlx::Postgres.*mock" crates/*/src/ crates/*/tests/ --include="*.rs" 2>/dev/null

# → testing-strategy.md § Behavior-Driven Rust Tests — no pub(crate) backdoors added for tests only
grep -rn "pub(crate)" crates/*/src/ --include="*.rs" | grep -B1 -A1 "cfg(test)"

# → testing-strategy.md § Rust Testing Trophy — required tool deps declared in workspace
grep -E "^(proptest|insta|wiremock|testcontainers) *=" Cargo.toml crates/*/Cargo.toml 2>/dev/null
```
