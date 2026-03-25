# Testing Strategy

> SSOT | **Last Updated**: 2026-03-18 | Classification: Operational

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

## Toolchain

### Rust

| Tool | Purpose | When |
|------|---------|------|
| `cargo nextest` | Parallel test execution | Always |
| `proptest` | Property-based testing (pure functions) | When writing units |
| `cargo-mutants` | Dead test detection | Once before release |
| `wiremock` | HTTP mock server for async client tests | When testing HTTP clients (e.g. MCP, provider adapters) |

### TypeScript (Web)

| Tool | Purpose | Config |
|------|---------|--------|
| vitest | Unit + Integration | `pool: threads`, `fileParallelism: true` |
| Playwright | E2E | `fullyParallel: true`, CI workers=4 |
| vitest-openapi | API schema validation | OpenAPI spec based |

### Bash E2E

| Pattern | Implementation |
|---------|---------------|
| Sequential | 01-setup → 03-inference (state creation) |
| Multi-model | 03-inference auto-detects available models and cycles through them for Round 2 + Goodput tests (multi-model parallel throughput) |
| Parallel 1 | 02, 04, 05, 06, 07, 11 concurrent execution (independent counts file) |
| Sequential | 08-sdd-advanced (clean state after parallel phases) |
| Parallel 2 | 09-metrics-pipeline, 10-image-storage |
| Verify + Liveness | 11-verify-liveness (pre-registration verify endpoints, heartbeat keys, online counter) |

`09-metrics-pipeline.sh` tests the full metrics pipeline end-to-end: verifies agent scrapes node-exporter, pushes via OTLP, data flows through Redpanda into ClickHouse, and the analytics API returns both gauge metrics (memory, GPU temp/power) and counter-derived metrics (CPU usage %). Tests both local (Mac) and remote (Ubuntu Ryzen AI 395+) server configurations.

---

## Adoption Plan

| Phase | Action | ROI |
|-------|--------|-----|
| **1** | OpenAPI schema validation → remove E2E duplication | High |
| **2** | proptest → pure functions (normalize, parse) | Medium |
| **3** | cargo-mutants one-time audit | Low (one-time) |

---

## Persistent Sample Data Policy

E2E test 실행 후 일부 데이터는 **수동 확인이 가능하도록 남겨둔다**.

### 원칙

| 구분 | 처리 |
|------|------|
| 임시 테스트 리소스 (CRUD lifecycle용) | 테스트 종료 즉시 삭제 |
| **대표 샘플 데이터** | **테스트 후에도 유지** — UI/API 직접 접근 가능 |

### 구현 방식

- 각 E2E 스크립트의 마지막 섹션에 **"Persistent Sample Data"** 블록을 작성한다.
- 블록은 동일 항목 중복 방지를 위해 **stale 데이터 정리 → 재등록** 순서로 실행한다.
- 샘플 데이터는 서비스 재시작 또는 DB 초기화 전까지 유지된다.
- 수동 확인 방법은 `pass` 메시지에 접근 경로를 명시한다 (예: `accessible at UI /mcp`).

### 적용 대상

| 리소스 | 샘플 데이터 | 유지 항목 |
|--------|------------|---------|
| MCP Servers | 날씨 MCP, 미세먼지 MCP 등록 후 미세먼지 삭제 | 날씨 MCP 1개 |
| (추후 확장) | 기타 핵심 리소스 | TBD |

---

## References

- [Testing Trophy — Kent C. Dodds](https://kentcdodds.com/blog/the-testing-trophy-and-testing-classifications)
- [Rust Testing Patterns 2026](https://dasroot.net/posts/2026/03/rust-testing-patterns-reliable-releases/)
- [proptest](https://docs.rs/proptest) | [cargo-mutants](https://mutants.rs/)
