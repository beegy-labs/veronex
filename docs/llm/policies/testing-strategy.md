# Testing Strategy

> SSOT | **Last Updated**: 2026-03-10

## Methodology: Testing Trophy + Contract Testing

Integration 테스트 중심, 중복 배제, 레이어별 책임 분리.

### Layer Responsibility (중복 금지)

| Layer | Verifies | Tool | Anti-Pattern |
|-------|----------|------|-------------|
| **Static** | Types, lint | TypeScript, Clippy | 타입으로 잡히는 건 테스트 안 씀 |
| **Unit** | 순수 함수 로직 | cargo test, vitest | HTTP/DB 검증 금지 |
| **Integration** | API 계약 (schema) | OpenAPI 검증, vitest | E2E와 같은 경로 중복 금지 |
| **E2E** | 사용자 흐름 | bash e2e, Playwright | 개별 함수 검증 금지 |

### Decision Checklist (테스트 작성 전)

```
1. 타입으로 잡히나?       → Yes → 테스트 불필요
2. 순수 함수인가?         → Yes → Unit (proptest 우선)
3. 외부 의존성?           → Yes → Integration (mock/schema)
4. 사용자 흐름?           → Yes → E2E (최소한만)
5. 다른 레이어에서 검증?  → Yes → 작성하지 않음
```

---

## Test Purity (순수성 원칙)

**"함수 수정 → unit만 깨짐 → E2E 불변"**

| 변경 유형 | Unit | Integration | E2E |
|----------|------|------------|-----|
| 내부 함수 로직 | FAIL | PASS | PASS |
| API 응답 스키마 | PASS | FAIL | FAIL |
| 사용자 흐름 | PASS | PASS | FAIL |

E2E가 내부 함수 변경에 깨지면 → **테스트 설계 결함** (레이어 침범).

---

## Toolchain

### Rust

| Tool | Purpose | When |
|------|---------|------|
| `cargo nextest` | 병렬 테스트 실행 | 항상 |
| `proptest` | Property-based testing (순수 함수) | unit 작성 시 |
| `cargo-mutants` | 죽은 테스트 식별 | 릴리스 전 1회 |

### TypeScript (Web)

| Tool | Purpose | Config |
|------|---------|--------|
| vitest | Unit + Integration | `pool: threads`, `fileParallelism: true` |
| Playwright | E2E | `fullyParallel: true`, CI workers=4 |
| vitest-openapi | API 스키마 검증 | OpenAPI spec 기반 |

### Bash E2E

| Pattern | Implementation |
|---------|---------------|
| Sequential | 01-setup → 02-inference (상태 생성) |
| Parallel | 03~06 동시 실행 (독립 counts file) |

---

## Adoption Plan

| Phase | Action | ROI |
|-------|--------|-----|
| **1** | OpenAPI 스키마 검증 → E2E 중복 제거 | 높음 |
| **2** | proptest → 순수 함수 (normalize, parse) | 중간 |
| **3** | cargo-mutants 1회 감사 | 낮음 (1회성) |

---

## References

- [Testing Trophy — Kent C. Dodds](https://kentcdodds.com/blog/the-testing-trophy-and-testing-classifications)
- [Rust Testing Patterns 2026](https://dasroot.net/posts/2026/03/rust-testing-patterns-reliable-releases/)
- [proptest](https://docs.rs/proptest) | [cargo-mutants](https://mutants.rs/)
