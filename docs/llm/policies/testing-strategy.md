# Testing Strategy

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational

## Methodology: Testing Trophy + Contract Testing

Integration-test focused, no duplication, clear layer responsibility separation.

### Layer Responsibility (No Duplication)

| Layer | Verifies | Tool | Anti-Pattern |
|-------|----------|------|-------------|
| **Static** | Types, lint | TypeScript, Clippy | Don't test what types already catch |
| **Unit** | Pure function logic | cargo test, vitest | No HTTP/DB verification |
| **Integration** | API contracts (schema) | OpenAPI validation, vitest | No overlap with E2E paths |
| **E2E** | User flows | bash e2e, Playwright | No individual function verification |

### Decision Checklist (Before Writing Tests)

```
1. Caught by types?              → Yes → No test needed
2. Pure function?                → Yes → Unit (proptest preferred)
3. External dependency?          → Yes → Integration (mock/schema)
4. User flow?                    → Yes → E2E (minimal only)
5. Already verified at another layer? → Yes → Don't write it
```

---

## Test Purity Principle

**"Function change → only unit breaks → E2E unchanged"**

| Change Type | Unit | Integration | E2E |
|------------|------|------------|-----|
| Internal function logic | FAIL | PASS | PASS |
| API response schema | PASS | FAIL | FAIL |
| User flow | PASS | PASS | FAIL |

If E2E breaks on internal function change → **test design flaw** (layer violation).

---

## Rust Testing Trophy (5-Layer, 2026)

Rust backends use a 5-layer structure tuned for typed pure cores + async I/O boundaries. Mix target: **Unit 50 / Integration 35 / Handler+E2E 15** — higher unit share than frontend because the type system and pure domain core catch more at compile time.

### Layer Responsibility

| Layer | Verifies | Tool | Environment | Anti-Pattern |
|-------|----------|------|-------------|--------------|
| **1. Static** | Types, lint, deps, licenses | `clippy --workspace -D warnings`, `cargo deny`, `cargo udeps` | — | Testing what the compiler / clippy already catches |
| **2. Unit** | Pure function + domain logic | `#[test]`, `#[tokio::test]`, **proptest**, **insta** | Process | HTTP / DB; mocks standing in for real external effects |
| **3. Integration** | Adapter × real external system | **testcontainers-rs** (Postgres/Valkey/Kafka), **wiremock** (HTTP) | Containers | Duplicating E2E flows; asserting on adapter internals |
| **4. Handler** | Axum handler contract (request → response) | `tower::ServiceExt::oneshot` + `axum::body::to_bytes` | Process | Spinning up a real HTTP server; going through `reqwest` |
| **5. E2E** | Cross-service flows, full docker-compose stack | `scripts/e2e/*.sh` (bash), later `just e2e` | Full stack | Asserting on individual function return values |

### Rust Test Purity

| Change Type | Unit | Integration | Handler | E2E |
|-------------|------|-------------|---------|-----|
| Domain / pure function logic | FAIL | PASS | PASS | PASS |
| DB schema / sqlx query | PASS | FAIL | PASS | FAIL |
| HTTP request/response shape | PASS | PASS | FAIL | FAIL |
| Cross-service / multi-crate flow | PASS | PASS | PASS | FAIL |

Cross-layer failures for a single-concern change = **test design flaw**. Fix the test, not the code under test.

### Behavior-Driven Rust Tests

All Rust tests assert on **observable behavior**, never implementation detail:

**Required:**
- Axum handlers: assert on HTTP status + response body JSON shape, not on inner function calls
- Domain services: assert on returned `Result` / emitted events, not on internal state
- Repositories: assert on persisted row count + row contents, not on the specific SQL text emitted

**Forbidden:**
- Mock call-count assertions as the primary verification (mock setup is fine; counting calls is implementation detail)
- Asserting on private module state via `pub(crate)` back-doors opened just for tests
- Snapshot tests of full struct debug output when only one field matters
- Asserting on log output or span names (use metrics for observable behavior)

### Axum Handler Test Pattern (Layer 4)

Direct `oneshot` against the `Router` — no HTTP server, no `reqwest`, deterministic and fast:

```rust
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[tokio::test]
async fn post_provider_returns_201() {
    let app = build_test_router(test_app_state()).await;

    let res = app
        .oneshot(
            Request::post("/v1/providers")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"x","url":"http://x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), 201);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("id").is_some());
}
```

Handler tests are the Rust equivalent of the frontend's Component layer — they exercise one inbound port with zero external systems running.

### Property-Based Unit Tests (`proptest`)

Every pure function with non-trivial input space (parsers, validators, normalizers, ID encoders) gets at least one `proptest`. The property should encode an invariant, not re-implement the function under test.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn encode_decode_round_trip(id in "[A-Za-z0-9_-]{16}") {
        let decoded = decode_id(&id)?;
        let re_encoded = encode_id(&decoded);
        prop_assert_eq!(re_encoded, id);
    }
}
```

### Snapshot Testing (`insta`)

Use `insta` only for **structural** snapshots where the output shape is genuinely stable (OpenAPI spec JSON, generated SQL migration order, error response bodies). Never snapshot whole struct dumps or free-form strings.

```rust
#[test]
fn openapi_spec_stable() {
    let spec = crate::openapi::build_spec();
    insta::assert_json_snapshot!(spec, {
        ".info.version" => "[version]",
    });
}
```

### Integration Testing with Real Services (`testcontainers-rs`)

Adapter tests (repository, message producer, Valkey Lua) spin up the **real** system in a container — never mock. A mocked Postgres is a different database with different bugs.

```rust
#[tokio::test]
async fn repo_upsert_persists() {
    let _pg = testcontainers::clients::Cli::default()
        .run(testcontainers::images::postgres::Postgres::default());
    // ... run migrations, exercise the repo, assert on row contents
}
```

### HTTP Client Adapter Tests (`wiremock`)

Outbound HTTP adapters (Ollama, Gemini, MCP) are tested against `wiremock` — real network, deterministic responses, verifies request shape *emitted* by the adapter.

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn ollama_chat_parses_response() {
    let mock = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"message\":{\"content\":\"hi\"}}"))
        .mount(&mock).await;

    let client = OllamaClient::new(mock.uri());
    let out = client.chat(&ChatRequest::test_default()).await.unwrap();
    assert_eq!(out.content, "hi");
}
```

### Mutation Testing (`cargo-mutants`)

Mutation testing runs in two modes:

| Mode | When | Command |
|------|------|---------|
| PR incremental | Every PR in CI | `cargo mutants --in-diff origin/develop --timeout 30` |
| Full sweep | Weekly nightly | `cargo mutants --timeout 60 --shard 1/4` (sharded across CI agents) |

Suppress trivial mutations with `#[mutants::skip]` on `Default::default()` / `String::new()` helpers — they produce noise, not signal. Disable `cargo-mutants` on crates that hit external services without a container fallback.

### Rust Decision Checklist (Before Writing a Test)

```
1. Caught by tsc-equivalent (type / clippy)? → Yes → No test needed
2. Pure function, domain logic, or hook?     → Yes → Unit (proptest for non-trivial input space)
3. Real Postgres / Valkey / Kafka / MCP?     → Yes → Integration (testcontainers)
4. Outbound HTTP to Ollama/Gemini?           → Yes → Integration (wiremock)
5. Axum handler request/response shape?      → Yes → Handler (oneshot)
6. Cross-service / cross-crate user flow?    → Yes → E2E (bash e2e)
7. Already verified at another layer?        → Yes → Do not duplicate
```

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

| Tool | Purpose | Config |
|------|---------|--------|
| vitest | Unit + Integration | `maxWorkers: N`, `fileParallelism: true` (v4+) |
| Playwright | E2E | `fullyParallel: true`, CI workers=4 |
| vitest-openapi | API schema validation | OpenAPI spec based |

### vitest v4 Config Changes

**Pool options moved to top level** (`poolOptions` removed):

```ts
// BEFORE (v3)
poolOptions: {
  threads: { maxThreads: 4, singleThread: true }
}

// AFTER (v4)
maxWorkers: 4,
isolate: false,   // replaces singleThread
```

**Environment assignment via `projects`** (`environmentMatchGlobs` removed):

```ts
// BEFORE (v3)
environmentMatchGlobs: [['**/*.spec.ts', 'jsdom']]

// AFTER (v4)
projects: [
  { test: { include: ['**/*.spec.ts'], environment: 'jsdom' } }
]
```

**Test options argument position changed**:

```ts
// BEFORE (v3)
test('name', () => {}, { retry: 2 })

// AFTER (v4)
test('name', { retry: 2 }, () => {})
```

**`done` callback removed** — use `async`/`await`:

```ts
// BEFORE
test('async', (done) => { done() })

// AFTER
test('async', async () => { await something() })
```

**Mock behavior changes**:
- `vi.restoreAllMocks()` no longer resets `vi.fn()` — add `vi.clearAllMocks()` explicitly if needed
- Mock default name changed from `'spy'` → `'vi.fn()'` — update any snapshots asserting on mock names
- Module factory must return an export object: `vi.mock('./x', () => ({ default: 'val' }))` (not bare value)

**Reporter changes**:
- `basic` reporter removed → use `{ reporter: 'default', summary: false }`

**Minimum requirements**: Node.js >= 20, Vite >= 6

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
- [Rust Testing Patterns 2026](https://dasroot.net/posts/2026/03/rust-testing-patterns-reliable-releases/)
- [Rust Integration Tests 2026](https://oneuptime.com/blog/post/2026-01-26-rust-integration-tests/view)
- [proptest](https://docs.rs/proptest)
- [insta — snapshot testing](https://insta.rs/)
- [testcontainers-rs](https://github.com/testcontainers/testcontainers-rs)
- [wiremock](https://github.com/LukeMathWalker/wiremock-rs)
- [cargo-mutants](https://mutants.rs/)
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/)
- [tower ServiceExt (Axum handler testing)](https://docs.rs/tower/latest/tower/trait.ServiceExt.html)
