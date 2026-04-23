# Backend Test

> ADD Execution — Rust Testing Trophy (5-Layer) | **Last Updated**: 2026-04-22

## Trigger

New Rust handler / domain service / repo / adapter; bug; pre-PR; refactor.

## Read Before Execution

| Doc | When |
|-----|------|
| `docs/llm/policies/testing-strategy.md § Rust Testing Trophy` | Always |
| `docs/llm/policies/patterns.md` | Always |
| `docs/llm/policies/architecture.md` | Layer boundaries |

## Layer Selection

Stop at the first Yes — do not double-cover.

| # | Question | Layer | Command |
|---|----------|-------|---------|
| 1 | Caught by types / `clippy -D warnings`? | Static | `cargo clippy --workspace -- -D warnings` |
| 2 | Pure function / domain / validator / parser? | Unit | `cargo nextest run -p <crate> --lib` |
| 3 | Real Postgres / Valkey / Kafka / MCP needed? | Integration | `cargo nextest run -p <crate> --test '*'` (testcontainers) |
| 4 | Outbound HTTP (Ollama / Gemini / MCP)? | Integration | `wiremock` in `#[tokio::test]` |
| 5 | Axum handler request → response contract? | Handler | `tower::ServiceExt::oneshot` in `#[tokio::test]` |
| 6 | Cross-service flow through docker-compose? | E2E | `bash scripts/e2e/NN-<name>.sh` |
| 7 | Already verified at another layer? | — | Don't write it |

## Writing Rules

| Rule | Detail |
|------|--------|
| Behavior-driven | Assert on returned values, HTTP response, persisted rows — not struct state or mock call counts |
| Handler form | `tower::ServiceExt::oneshot` + `axum::body::to_bytes`. No `reqwest`, no real HTTP server |
| Real systems | `testcontainers-rs` for DB/queue; never mock Postgres / Valkey |
| HTTP mock | `wiremock` — assert on request shape emitted by adapter |
| Property-based | `proptest` on pure fns with non-trivial input space |
| Snapshots | `insta` only for stable outputs (OpenAPI spec, migration order, error bodies) |
| No backdoors | Never add `pub(crate)` solely for a test — expose via a public port instead |
| Async | `#[tokio::test(flavor = "multi_thread")]` for concurrency tests |
| Fixtures | Explicit insert helpers in `tests/support/`, not global `static` state |

## Steps

| # | Action |
|---|--------|
| 1 | Pick layer via checklist |
| 2 | Read neighboring tests in same crate to match style |
| 3 | Write using the layer's required form |
| 4 | Run the single layer with `--project` / `-p` scope |
| 5 | Verify purity: rename an internal helper — only Unit tests should fail |
| 6 | E2E: run `bash scripts/e2e/NN-<name>.sh` before pushing |

## Purity Verification

- Internal fn rename → only Unit fails
- DB schema change → only Integration fails
- HTTP response shape change → only Handler fails
- Cross-service flow change → only E2E fails

Cross-layer failures from a single-concern change = **test design flaw** → rewrite.

## Forbidden Patterns

| Pattern | Reason |
|---------|--------|
| `reqwest` against a local Axum server in unit/handler tests | Use `oneshot` — deterministic, no port bind |
| In-memory fake Postgres / Valkey | Use `testcontainers-rs` |
| `#[should_panic]` as primary assertion | Assert on `Err` variant |
| `assert!(result.is_ok())` | Pattern-match or `unwrap` to a concrete value |
| `dbg!` / `println!` left committed | Test pollution |
| Whole-struct `Debug` snapshot | Hides intent; brittle |
| `assert!(mock.calls() == N)` as primary | Use `wiremock::Mock::expect(N)` only when contract requires; assert on side effects otherwise |
| `pub(crate)` opened only for a test | Refactor to a public port |
| Hardcoded `thread::sleep` waits | Use `tokio::time::timeout` + signals |

## Crate Coverage Requirements

| Crate | Layers required |
|-------|-----------------|
| `veronex` | Unit + Integration + Handler + E2E |
| `veronex-mcp` | Unit + Integration (wiremock) + Handler |
| `veronex-agent` | Unit + Integration (wiremock for OTLP) |
| `veronex-analytics` | Unit + Integration + Handler |
| `veronex-consumer` | Unit + Integration (testcontainers Kafka + ClickHouse) |
| `veronex-embed` | Unit + Handler |

## Mandatory Proptest Targets

- ID encoder/decoder (round-trip)
- URL normalizer / SSRF validator (idempotency + allowed-only-if-parsed)
- Duration / size / rate parser
- Domain enum parser (`parse_db_enum`)
- UTF-8 safe truncation (length bound + UTF-8 validity)

## Checklist

- [ ] Correct layer selected
- [ ] Behavior-driven assertions only
- [ ] Handler uses `oneshot`
- [ ] Integration uses `testcontainers-rs` / `wiremock`
- [ ] Purity verified
- [ ] `cargo nextest run -p <crate>` + `clippy -D warnings` pass
- [ ] Proptest added where mandated
- [ ] No `pub(crate)` test backdoors
