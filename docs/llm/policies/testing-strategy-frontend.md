# Testing Strategy — Frontend

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent: `testing-strategy.md` (common 5-Layer methodology, purity, toolchain)

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
