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

## Behavior-Driven Tests (user-centric)

All frontend tests — Unit, Component, E2E — test **observable behavior**, never implementation details.

**Forbidden (implementation-detail tests):**

- Asserting on private function internals (`expect(internalHelper).toHaveBeenCalled()`)
- Querying by CSS class or generated data attributes (`container.querySelector('.btn-submit')`)
- Asserting on React component instance state, refs, or props from outside the component
- Snapshot tests of full DOM trees (use targeted assertions instead)
- Mock call-count assertions as the primary verification (mock setup is fine; asserting on calls is not)

**Required (behavior tests):**

- Query the DOM the way a user / assistive tech would
- Assert on what the user sees, hears, or navigates to
- After a refactor that preserves behavior, all tests must still pass without edits

> *"The more your tests resemble the way your software is used, the more confidence they can give you."* — Testing Library docs

Synonyms (all equivalent): **behavior-driven tests**, **user-centric tests**, **black-box tests**, **refactor-resistant tests**. Use *behavior-driven* or *user-centric* in code comments and docs; *refactor-resistant* is a property, not the name of the technique.

---

## Testing Library Query Priority

Use queries in this order. Drop to a lower tier only with a comment explaining why.

| Tier | Query | When |
|------|-------|------|
| 1 (preferred) | `getByRole`, `getByLabelText`, `getByPlaceholderText` | Interactive elements — form fields, buttons, links |
| 2 | `getByText`, `getByDisplayValue`, `getByAltText`, `getByTitle` | Static content, images |
| 3 (last resort) | `getByTestId` | Element with no role, no accessible name, and no stable visible text |

### `getByRole` Performance Exception

`getByRole` runs the full accessibility tree and is 100–1000× slower than `getByText` / `getByLabelText` in jsdom. For non-interactive read-only assertions on static content, `getByText` is acceptable even when `getByRole` would work. This exception does NOT apply to:

- Interactive elements (button, textbox, link, checkbox, etc.) — always `getByRole`
- Assertions that verify accessible naming (`aria-label`, `aria-labelledby`)
- Component tests running in Vitest Browser Mode (where real browser a11y tree is fast)

Add an inline comment when dropping from `getByRole`: `// perf: getByText avoids full a11y tree`.

### Forbidden Queries

- `document.querySelector` / `container.querySelector` — implementation detail
- `getByClassName` / class-based selectors — implementation detail
- Custom data attributes beyond `data-testid` — bypasses the priority ladder

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

| Tool | Purpose | Layer | Config |
|------|---------|-------|--------|
| vitest (jsdom) | Pure function + hook logic | Unit | `environment: 'jsdom'` project |
| **vitest Browser Mode** | Single-component render + interaction | **Component** | `browser: { enabled: true, provider: 'playwright' }` project |
| vitest-openapi | API schema validation | Integration | OpenAPI spec based |
| Playwright | Multi-page user flows | E2E | `fullyParallel: true`, CI workers=4 |

**jsdom is forbidden for layout / visual / focus / scroll / CSS assertions.** Any such test must be a Component test in Browser Mode. Rationale: jsdom does not implement CSSOM, layout, or real focus traversal — tests that appear to pass in jsdom may reflect jsdom bugs rather than application behavior.

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

| Phase | Action | ROI |
|-------|--------|-----|
| **1** | OpenAPI schema validation → remove E2E duplication | High |
| **2** | proptest → pure functions (normalize, parse) | Medium |
| **3** | Vitest Browser Mode project for Component layer | High |
| **4** | Migrate layout / focus / CSS assertions from jsdom → Browser Mode | High |
| **5** | cargo-mutants one-time audit | Low (one-time) |

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
- [proptest](https://docs.rs/proptest) | [cargo-mutants](https://mutants.rs/)
