# Backend Test

> ADD Execution — Rust Testing Trophy (5-Layer) | **Last Updated**: 2026-04-22

## Trigger

- New Rust handler / domain service / repository / adapter
- Rust bug reported
- Pre-commit / pre-PR verification
- Refactor that must remain behavior-compatible

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Testing SSOT | `docs/llm/policies/testing-strategy.md § Rust Testing Trophy` | Always — layer responsibility, purity, behavior-driven rules |
| Rust patterns | `docs/llm/policies/patterns.md` | Always |
| Architecture | `docs/llm/policies/architecture.md` | Layer boundaries |

## Layer Selection

Run through the checklist in order. Stop at the first Yes — do not double-cover.

| # | Question | Layer | Command |
|---|----------|-------|---------|
| 1 | Already caught by `cargo check` / `cargo clippy -D warnings` / types? | Static | `cargo clippy --workspace -- -D warnings` |
| 2 | Pure function / domain logic / validator / parser / normalizer? | **Unit** | `cargo nextest run -p <crate> --lib` |
| 3 | Real Postgres / Valkey / Kafka / MCP server needed? | **Integration** | `cargo nextest run -p <crate> --test '*'` (with `testcontainers`) |
| 4 | Outbound HTTP to Ollama / Gemini / MCP? | **Integration** | `wiremock` inside a `#[tokio::test]` |
| 5 | Axum handler request → response contract? | **Handler** | `tower::ServiceExt::oneshot` in `#[tokio::test]` |
| 6 | Cross-service / cross-crate flow through docker-compose stack? | **E2E** | `bash scripts/e2e/NN-<name>.sh` |
| 7 | Already verified at another layer? | — | Do not write the test |

## Test Writing Rules

All layers follow behavior-driven testing.

| Rule | Applies to | Detail |
|------|-----------|--------|
| Behavior-driven | All | Assert on returned values, HTTP response shape, persisted rows, emitted events — never on internal struct state or mock call counts |
| Handler test form | Handler | `tower::ServiceExt::oneshot` + `axum::body::to_bytes`. No `reqwest`, no `TcpListener`, no real HTTP server |
| Real DB/Queue | Integration | `testcontainers-rs` — never mock Postgres / Valkey / Kafka |
| HTTP client mocking | Integration | `wiremock` — assert on the request shape emitted by the adapter |
| Property-based | Unit | `proptest` for any pure function with non-trivial input space |
| Snapshot scope | Unit / Handler | `insta` only for stable-shape outputs (OpenAPI spec, migration order, error bodies). No whole-struct Debug snapshots |
| No private backdoors | All | Never add `pub(crate)` solely to enable a test — refactor to a public port instead |
| Async tests | Handler / Integration | `#[tokio::test]` with `flavor = "multi_thread"` for concurrency tests |
| Fixtures | All | Seed data via explicit insert helpers in `tests/support/`, not global `static` state |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Classify the change → pick the Layer via the selection checklist |
| 2 | Read existing tests in the same crate to match style |
| 3 | Write the test using the form required by the layer |
| 4 | Run the single layer: `cargo nextest run -p <crate> --lib` (Unit) / `cargo nextest run -p <crate> --test '*'` (Integration/Handler) |
| 5 | Verify purity: rename an internal helper — only Unit tests should fail |
| 6 | For E2E: run `bash scripts/e2e/NN-<name>.sh` locally before pushing |

## Test Purity Verification

Any PR that adds or changes a test must satisfy:

- Internal function rename → only Unit tests fail
- DB schema change → only Integration tests fail
- HTTP response shape change → only Handler tests fail
- Cross-service flow change → only E2E tests fail

Cross-layer failures from a single-concern change = **test design flaw** → rewrite before merging.

## Forbidden Patterns

| Pattern | Reason |
|---------|--------|
| `reqwest` against a locally-spawned Axum server for unit/handler tests | Use `tower::ServiceExt::oneshot` — faster, deterministic, no port bind |
| Mocked Postgres / Valkey via in-memory fake | Use `testcontainers-rs` with real Postgres / Valkey |
| `#[should_panic]` as the primary assertion | Assert on the returned `Err` variant instead |
| `assert!(result.is_ok())` | Use `assert_eq!(result.unwrap(), expected)` or pattern-match the `Ok(x)` value |
| `dbg!` / `println!` left in committed tests | Test pollution |
| Whole-struct `#[derive(Debug)]` snapshots | Hides intent; brittle on unrelated field changes |
| `assert!(mock.calls() == N)` as primary assertion | Use `wiremock::Mock::expect(N)` only when the adapter's contract requires exactly N calls; for everything else, assert on side effects |
| Opening a `pub(crate)` visibility solely for a test | Refactor to expose the behavior via a public port |
| Hardcoded `thread::sleep(Duration::from_secs(1))` waits | Use `tokio::time::timeout` + deterministic signals |

## Crate-Level Coverage Requirements

| Crate | Layers required | Rationale |
|-------|-----------------|-----------|
| `veronex` (API + scheduler) | Unit + Integration + Handler + E2E | Core service, full surface |
| `veronex-mcp` | Unit + Integration (wiremock) + Handler | MCP server library |
| `veronex-agent` | Unit + Integration (wiremock for OTLP) | Outbound only; no inbound HTTP |
| `veronex-analytics` | Unit + Integration + Handler | ClickHouse-backed API |
| `veronex-consumer` | Unit + Integration (testcontainers Kafka + ClickHouse) | OTLP parse + persist |
| `veronex-embed` | Unit + Handler | Stateless embedding service |

## Proptest Targets (mandatory for 10K scale)

These modules MUST have at least one `proptest`:

- Any ID encoder/decoder (round-trip property)
- URL normalizer / SSRF validator (idempotency + allowed-only-if-parsed)
- Duration / size / rate parser
- Domain enum parser (`parse_db_enum`)
- Any UTF-8 safe truncation helper (length bound + UTF-8 validity)

## Output Checklist

- [ ] Correct layer selected via checklist
- [ ] Behavior-driven assertions only
- [ ] Handler test uses `oneshot` (not `reqwest`)
- [ ] Integration test uses `testcontainers-rs` / `wiremock` (not mocks)
- [ ] Purity verified: internal rename doesn't cross layers
- [ ] `cargo nextest run -p <crate>` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] Proptest added where mandated
- [ ] No `pub(crate)` test backdoors introduced
