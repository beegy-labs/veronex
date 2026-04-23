# Testing Strategy

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational

## Methodology: Testing Trophy (5-Layer, 2026)

Behavior-driven, integration-heavy, zero cross-layer duplication. *"Write tests. Not too many. Mostly integration."* — Kent C. Dodds.

### Layer Responsibility (No Duplication)

| Layer | Verifies | Tool | Environment | Anti-Pattern |
|-------|----------|------|-------------|--------------|
| **1. Static** | Types, lint | TypeScript, Clippy, ESLint | — | Testing what the type system already catches |
| **2. Unit** | Pure function / hook logic | vitest + jsdom, cargo test, proptest | Node | HTTP/DB verification; mocking your way around integration concerns |
| **3. Component** | Single-component render + user interaction | **Vitest Browser Mode**, Playwright Component Testing | Real browser | jsdom for visual / layout-dependent behavior; testing implementation details |
| **4. Integration** | API contracts (schema), cross-module wiring | vitest + OpenAPI validation, wiremock | Node + mock server | Duplicating E2E user flows |
| **5. E2E** | End-to-end user flows | Playwright, bash e2e | Real browser + real backend | Asserting on individual function return values |

**Frontend stack**: Unit in jsdom, Component in Vitest Browser Mode, E2E in Playwright. jsdom is restricted to pure logic — any test that asserts on layout, focus, scrolling, or CSS must run in a real browser.

### Decision Checklist (Before Writing a Test)

```
1. Caught by types?                      → Yes → No test needed
2. Pure function or hook logic?          → Yes → Unit (proptest for pure, RTL renderHook for hooks)
3. Single component render/interaction?  → Yes → Component (Vitest Browser Mode)
4. API contract / cross-module wiring?   → Yes → Integration (schema validation or wiremock)
5. Multi-page user flow?                 → Yes → E2E (minimal set only)
6. Already verified at another layer?    → Yes → Don't write it
```

---

## Test Purity Principle

**"A change in one layer must break tests in that layer only."**

| Change Type | Unit | Component | Integration | E2E |
|-------------|------|-----------|-------------|-----|
| Internal function / hook logic | FAIL | PASS | PASS | PASS |
| Component markup / interaction | PASS | FAIL | PASS | PASS |
| API response schema | PASS | PASS | FAIL | FAIL |
| Multi-page user flow | PASS | PASS | PASS | FAIL |

Cross-layer failures = **layer violation** = test design flaw. Fix the tests, not the code.

---

## Frontend Testing

Frontend-specific rules (behavior-driven, Testing Library query priority, `getByRole` performance exception, forbidden queries) are in **[`testing-strategy-frontend.md`](testing-strategy-frontend.md)**.

---

## Rust Testing

Rust-specific testing rules (5-Layer Trophy, Axum `oneshot` handler tests, proptest / insta / testcontainers / wiremock, cargo-mutants, behavior-driven rules) are in **[`testing-strategy-rust.md`](testing-strategy-rust.md)**.

---

## Toolchain

### Rust

| Tool | Purpose | Layer | When |
|------|---------|-------|------|
| `cargo nextest` | Parallel test execution | All | Always |
| `cargo clippy -D warnings` | Lint as error | Static | Pre-commit |
| `cargo deny check` | License + advisory + duplicate crate audit | Static | CI |
| `cargo udeps` | Unused dependency detection | Static | Pre-release |
| **proptest** | Property-based testing for pure functions | Unit | Non-trivial input space |
| **insta** | Structural snapshots (OpenAPI spec, migration order) | Unit/Handler | Stable-shape outputs only |
| **testcontainers-rs** | Real Postgres / Valkey / Kafka in a container | Integration | Repository / queue / Lua tests |
| **wiremock** | HTTP mock for outbound adapters | Integration | Ollama / Gemini / MCP / auth providers |
| **tower `ServiceExt::oneshot`** | Direct Axum handler invocation | Handler | All inbound HTTP handler tests |
| **axum `body::to_bytes`** | Extract handler response bodies | Handler | Assert on response JSON |
| **cargo-mutants** | Mutation testing | Meta | PR `--in-diff`; weekly full sweep |

All crates in `crates/` MUST have at least Unit + Handler (if they expose HTTP) coverage. Integration coverage is required for any crate that touches a DB, queue, or outbound HTTP adapter.

### TypeScript (Web)

| Tool | Purpose | Layer | Config |
|------|---------|-------|--------|
| vitest (jsdom) | Pure function + hook logic | Unit | `environment: 'jsdom'` project |
| **vitest Browser Mode** | Single-component render + interaction | **Component** | `browser: { enabled: true, provider: 'playwright' }` project |
| vitest-openapi | API schema validation | Integration | OpenAPI spec based |
| Playwright | Multi-page user flows | E2E | `fullyParallel: true`, CI workers=4 |

**jsdom is forbidden for layout / visual / focus / scroll / CSS assertions.** Any such test must be a Component test in Browser Mode. Rationale: jsdom does not implement CSSOM, layout, or real focus traversal — tests that appear to pass in jsdom may reflect jsdom bugs rather than application behavior.

### vitest v4 Notes

v3→v4 migration details (poolOptions, projects, options position, mock behavior, reporters, min reqs) → `docs/llm/research/frontend/vitest-v4-migration.md`.

### Bash E2E

| Wave | Scripts | Mode | Notes |
|------|---------|------|-------|
| Phase 0 | `01-setup` | sequential | DB reset + infra bootstrap |
| Wave 1 | `05` `09` `13` | **parallel** | read-only / fully isolated |
| Wave 2 | `04` `06` `10` `12` `15` `17` | **parallel** | own resources; MCP/run-id isolated |
| Wave 3 | `02` `03` `07` `08` `16` `14` | sequential | share AIMD + provider state; 16 patches global lab settings |

Multi-model: `03-inference` auto-detects available models and cycles through them for Round 2 + Goodput tests (multi-model parallel throughput).

Verify + Liveness: merged into `04-crud` — tests pre-registration verify endpoints (server/provider URL validation), heartbeat keys, online counter.

`09-metrics-pipeline.sh` tests the full metrics pipeline end-to-end: verifies agent scrapes node-exporter, pushes via OTLP, data flows through Redpanda **→ veronex-consumer → ClickHouse**, and the analytics API returns both gauge metrics (memory, GPU temp/power) and counter-derived metrics (CPU usage %). Tests both local (Mac) and remote (Ubuntu Ryzen AI 395+) server configurations.

**veronex-consumer unit tests** (`cargo test -p veronex-consumer`):

| Module | Coverage |
|--------|----------|
| `handlers::logs` | inference routing, audit routing, mcp_tool_calls routing, unknown event drop, empty payload, empty resourceLogs |
| `handlers::metrics` | gauge datapoints, sum datapoints, empty payload, multi-resource |
| `handlers::traces` | raw payload storage, empty resourceSpans |

Unit tests verify pure OTLP parse → row mapping logic only (no Kafka/ClickHouse I/O). Integration coverage comes from `09-metrics-pipeline.sh` which confirms data actually reaches ClickHouse through the full pipeline.

---

## Adoption Plan

### Frontend

| Phase | Action | ROI |
|-------|--------|-----|
| **F1** | OpenAPI schema validation → remove E2E duplication | High |
| **F2** | Vitest Browser Mode project for Component layer | High |
| **F3** | Migrate layout / focus / CSS assertions from jsdom → Browser Mode | High |

### Rust

| Phase | Action | ROI |
|-------|--------|-----|
| **R1** | Add `proptest` dep + convert ≥5 pure modules (ID encoder, URL normalizer, validator) | High |
| **R2** | Add `wiremock` dep + wrap every outbound HTTP adapter (Ollama, Gemini, MCP) | High |
| **R3** | Add `testcontainers-rs` + convert at least one repository integration test | High |
| **R4** | Introduce Handler-layer test pattern (oneshot) — migrate existing HTTP tests | High |
| **R5** | Add `insta` for OpenAPI snapshot; enable `cargo-mutants --in-diff` in CI | Medium |
| **R6** | Add `cargo deny` + `cargo udeps` to CI | Medium |

---

## Persistent Sample Data Policy

Some data is intentionally **kept after E2E tests for manual verification**.

### Principles

| Category | Handling |
|----------|----------|
| Temporary test resources (CRUD lifecycle) | Deleted immediately after test |
| **Representative sample data** | **Persisted after test** -- directly accessible via UI/API |

### Implementation

- Add a **"Persistent Sample Data"** block at the end of each E2E script.
- The block runs **stale data cleanup -> re-register** to prevent duplicates.
- Sample data persists until service restart or DB reset.
- Include the access path in the `pass` message (e.g., `accessible at UI /mcp`).

### Scope

| Resource | Sample Data | Retained |
|----------|-------------|----------|
| MCP Servers | Register Weather MCP + Air Quality MCP, then delete Air Quality | 1 Weather MCP |
| (future) | Other core resources | TBD |

---

## References

- [Testing Trophy — Kent C. Dodds](https://kentcdodds.com/blog/the-testing-trophy-and-testing-classifications)
- [Write tests. Not too many. Mostly integration. — Kent C. Dodds](https://kentcdodds.com/blog/write-tests)
- [Avoid Testing Implementation Details — Kent C. Dodds](https://kentcdodds.com/blog/testing-implementation-details)
- [Why I Won't Use jsdom — Kent C. Dodds / Epic Web](https://www.epicweb.dev/why-i-won-t-use-jsdom)
- [Vitest Browser Mode](https://vitest.dev/guide/browser/why)
- [Testing Library — Query Priority](https://testing-library.com/docs/queries/about/#priority)
- [Testing Library — Guiding Principles](https://testing-library.com/docs/guiding-principles)
- [Playwright Component Testing](https://playwright.dev/docs/test-components)
- [Next.js Testing with Vitest](https://nextjs.org/docs/app/guides/testing/vitest)
- [Rust Testing Patterns 2026](https://dasroot.net/posts/2026/03/rust-testing-patterns-reliable-releases/)
- [Rust Integration Tests 2026](https://oneuptime.com/blog/post/2026-01-26-rust-integration-tests/view)
- [proptest](https://docs.rs/proptest)
- [insta — snapshot testing](https://insta.rs/)
- [testcontainers-rs](https://github.com/testcontainers/testcontainers-rs)
- [wiremock](https://github.com/LukeMathWalker/wiremock-rs)
- [cargo-mutants](https://mutants.rs/)
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/)
- [tower ServiceExt (Axum handler testing)](https://docs.rs/tower/latest/tower/trait.ServiceExt.html)
