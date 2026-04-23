# Testing Strategy — Rust

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent: `testing-strategy.md` (common methodology, frontend layers, toolchain, references)

## Rust Testing Trophy (5-Layer)

Rust backends use a 5-layer structure tuned for typed pure cores + async I/O boundaries. Mix target: **Unit 50 / Integration 35 / Handler+E2E 15** — higher unit share than frontend because the type system and pure domain core catch more at compile time.

### Layer Responsibility

| Layer | Verifies | Tool | Environment | Anti-Pattern |
|-------|----------|------|-------------|--------------|
| **1. Static** | Types, lint, deps, licenses | `clippy -D warnings`, `cargo deny`, `cargo udeps` | — | Testing what clippy already catches |
| **2. Unit** | Pure function + domain logic | `#[test]`, `#[tokio::test]`, **proptest**, **insta** | Process | HTTP / DB; mocks standing in for real external effects |
| **3. Integration** | Adapter × real external system | **testcontainers-rs** (Postgres/Valkey/Kafka), **wiremock** (HTTP) | Containers | Duplicating E2E flows; asserting on adapter internals |
| **4. Handler** | Axum handler contract (request → response) | `tower::ServiceExt::oneshot` + `axum::body::to_bytes` | Process | Real HTTP server; going through `reqwest` |
| **5. E2E** | Cross-service flows, full docker-compose | `scripts/e2e/*.sh` bash, later `just e2e` | Full stack | Asserting on individual function return values |

## Rust Test Purity

| Change Type | Unit | Integration | Handler | E2E |
|-------------|------|-------------|---------|-----|
| Domain / pure function logic | FAIL | PASS | PASS | PASS |
| DB schema / sqlx query | PASS | FAIL | PASS | FAIL |
| HTTP request/response shape | PASS | PASS | FAIL | FAIL |
| Cross-service / multi-crate flow | PASS | PASS | PASS | FAIL |

Cross-layer failures for a single-concern change = **test design flaw**. Fix the test, not the code under test.

## Behavior-Driven Rust Tests

Assert on **observable behavior**, never implementation detail.

**Required:**
- Axum handlers: HTTP status + response body JSON shape, not inner function calls
- Domain services: returned `Result` / emitted events, not internal state
- Repositories: persisted row count + contents, not the SQL text emitted

**Forbidden:**
- Mock call-count assertions as primary verification
- `pub(crate)` back-doors opened solely for tests
- Snapshot tests of full struct debug output when one field matters
- Asserting on log output or span names (use metrics instead)

## Axum Handler Test Pattern (Layer 4)

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
        .await.unwrap();
    assert_eq!(res.status(), 201);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("id").is_some());
}
```

Handler tests are the Rust equivalent of the frontend's Component layer — exercise one inbound port with zero external systems running.

## Property-Based Unit Tests (`proptest`)

Every pure function with non-trivial input space (parsers, validators, normalizers, ID encoders) gets at least one `proptest`. The property encodes an invariant, never re-implements the function.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn encode_decode_round_trip(id in "[A-Za-z0-9_-]{16}") {
        let decoded = decode_id(&id)?;
        prop_assert_eq!(encode_id(&decoded), id);
    }
}
```

## Snapshot Testing (`insta`)

Use `insta` only for **structural** snapshots where the output shape is genuinely stable (OpenAPI spec JSON, migration order, error response bodies). Never snapshot whole struct dumps or free-form strings.

```rust
#[test]
fn openapi_spec_stable() {
    let spec = crate::openapi::build_spec();
    insta::assert_json_snapshot!(spec, { ".info.version" => "[version]" });
}
```

## Integration Testing (`testcontainers-rs`)

Adapter tests (repository, message producer, Valkey Lua) spin up the **real** system — never mock. A mocked Postgres is a different database with different bugs.

```rust
#[tokio::test]
async fn repo_upsert_persists() {
    let _pg = testcontainers::clients::Cli::default()
        .run(testcontainers::images::postgres::Postgres::default());
    // run migrations, exercise repo, assert on row contents
}
```

## HTTP Client Adapter Tests (`wiremock`)

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

## Mutation Testing (`cargo-mutants`)

| Mode | When | Command |
|------|------|---------|
| PR incremental | Every PR in CI | `cargo mutants --in-diff origin/develop --timeout 30` |
| Full sweep | Weekly nightly | `cargo mutants --timeout 60 --shard 1/4` |

Suppress trivial mutations with `#[mutants::skip]` on `Default::default()` / `String::new()` helpers. Disable on crates that hit external services without a container fallback.

## Rust Decision Checklist

```
1. Caught by types / clippy -D warnings? → Yes → No test
2. Pure function / domain / hook?         → Yes → Unit (proptest for non-trivial)
3. Real Postgres / Valkey / Kafka / MCP?  → Yes → Integration (testcontainers)
4. Outbound HTTP (Ollama/Gemini)?         → Yes → Integration (wiremock)
5. Axum handler request/response shape?   → Yes → Handler (oneshot)
6. Cross-service / multi-crate flow?      → Yes → E2E (bash e2e)
7. Already verified at another layer?     → Yes → Don't duplicate
```
