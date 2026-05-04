# Frontend Test

> ADD Execution — Testing Trophy (5-Layer) | **Last Updated**: 2026-04-22

## Trigger

- New frontend feature / component / hook
- Frontend bug reported
- Pre-commit / pre-PR verification
- Refactor that must remain behavior-compatible

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Testing SSOT | `docs/llm/policies/testing-strategy.md` | Always — layer responsibility, purity, behavior-driven rules |
| Frontend patterns | `docs/llm/policies/patterns-frontend.md` | Always |
| Execution contracts | `docs/llm/frontend/execution-contracts.md` | Feature boundaries, realtime/error contracts |

## Layer Selection

Run through the checklist in order. Stop at the first Yes — do not double-cover.

| # | Question | Layer | Command |
|---|----------|-------|---------|
| 1 | Already caught by `tsc --noEmit` / lint? | Static | `npx tsc --noEmit` |
| 2 | Pure function / hook logic with no DOM? | **Unit** | `npx vitest run --project unit` |
| 3 | Single component render, user interaction, or layout-aware behavior (focus, scroll, CSS)? | **Component** | `npx vitest run --project component` (Vitest Browser Mode) |
| 4 | API schema contract or cross-module wiring? | **Integration** | `npx vitest run --project integration` |
| 5 | Multi-page user flow across routes? | **E2E** | `npx playwright test` |
| 6 | Already verified at another layer? | — | Do not write the test |

## Test Writing Rules

All layers follow behavior-driven testing. Tests describe what the user observes, not how the code is organized.

| Rule | Applies to | Detail |
|------|-----------|--------|
| Behavior-driven | Unit, Component, E2E | No private-fn assertions, no CSS-class queries, no full-DOM snapshots, no mock-call-count as primary assertion |
| Query priority | Component, E2E | `getByRole` > `getByLabelText` > `getByText` > `getByTestId`. Drop only with inline comment explaining why |
| `getByRole` performance | Unit (jsdom) only | Permitted to use `getByText` in jsdom for static non-interactive content — add `// perf: getByText avoids full a11y tree` comment |
| jsdom scope | Unit | Pure function / hook logic only. Never layout, focus, scroll, CSS, or visual assertions |
| Browser Mode scope | Component | Anything DOM-visual. Replaces the old RTL+jsdom pattern |
| i18n in tests | Component, E2E | Use same `t()` keys the UI uses — never hardcode translated strings |
| E2E resource cleanup | E2E | `try/finally` with API-based cleanup on failure path |
| Test names | All | Describe observable behavior (`"disables submit when name is empty"`), not implementation (`"calls setDisabled with true"`) |

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Classify the change → pick the Layer via the selection checklist above |
| 2 | Read the target layer's examples in existing tests (`web/lib/__tests__/` for Unit/Integration) |
| 3 | Write the test using the query priority and behavior-driven rules |
| 4 | Run the single layer: `npx vitest run --project <name>` — must pass before moving on |
| 5 | If the change crosses layers (rare), run each affected project separately; never run all tests blindly |
| 6 | Verify purity: make a trivial internal rename of the function under test — only the intended layer should fail |
| 7 | For E2E: run Playwright locally (`npx playwright test`) before pushing |

## Test Purity Verification

Any PR that adds or changes a test must satisfy:

- Internal function rename → only Unit tests fail
- Component markup refactor that preserves rendered output → no tests fail
- API schema change → only Integration tests fail
- User-visible flow change → only E2E tests fail

If a test fails outside the expected layer for a non-behavioral change, the test is implementation-coupled → **rewrite it before merging**.

## Forbidden Patterns

| Pattern | Reason |
|---------|--------|
| `container.querySelector(...)` | Implementation detail |
| `getByClassName`, class-selector queries | Implementation detail |
| Full-tree snapshot tests | Brittle, hides intent |
| Asserting on `useState` / `useRef` internals | Implementation detail |
| `expect(mock).toHaveBeenCalledTimes(N)` as the primary assertion | Implementation detail — assert on the resulting user-visible effect instead |
| Layout / focus / scroll assertions in jsdom | jsdom lacks CSSOM and real layout — results are unreliable |
| `screen.debug()` left in committed code | Test pollution |
| Hardcoded translated strings | Breaks when i18n changes |

## When No Test Is Needed

- Pure TypeScript type definitions (types are verified by `tsc`)
- Thin wrappers around library components with no added logic
- Code paths already covered by a higher/lower layer (Test Purity: never duplicate)

## Output Checklist

- [ ] Correct layer selected via checklist
- [ ] Behavior-driven assertions only
- [ ] Query priority respected (or fallback commented)
- [ ] jsdom limitations honored (layout/focus/CSS → Browser Mode)
- [ ] Purity verified: internal rename does not cross layers
- [ ] Project-scoped command passes (`--project <name>`)
- [ ] `tsc --noEmit` passes
