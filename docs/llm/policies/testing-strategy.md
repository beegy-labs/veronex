# Testing Strategy

> SSOT | **Last Updated**: 2026-03-10

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

### TypeScript (Web)

| Tool | Purpose | Config |
|------|---------|--------|
| vitest | Unit + Integration | `pool: threads`, `fileParallelism: true` |
| Playwright | E2E | `fullyParallel: true`, CI workers=4 |
| vitest-openapi | API schema validation | OpenAPI spec based |

### Bash E2E

| Pattern | Implementation |
|---------|---------------|
| Sequential | 01-setup → 02-inference (state creation) |
| Parallel | 03~06 concurrent execution (independent counts file) |

---

## Adoption Plan

| Phase | Action | ROI |
|-------|--------|-----|
| **1** | OpenAPI schema validation → remove E2E duplication | High |
| **2** | proptest → pure functions (normalize, parse) | Medium |
| **3** | cargo-mutants one-time audit | Low (one-time) |

---

## References

- [Testing Trophy — Kent C. Dodds](https://kentcdodds.com/blog/the-testing-trophy-and-testing-classifications)
- [Rust Testing Patterns 2026](https://dasroot.net/posts/2026/03/rust-testing-patterns-reliable-releases/)
- [proptest](https://docs.rs/proptest) | [cargo-mutants](https://mutants.rs/)
