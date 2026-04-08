# Testing Strategy

> SSOT | **Last Updated**: 2026-03-25 | Classification: Operational

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
- [Rust Testing Patterns 2026](https://dasroot.net/posts/2026/03/rust-testing-patterns-reliable-releases/)
- [proptest](https://docs.rs/proptest) | [cargo-mutants](https://mutants.rs/)
