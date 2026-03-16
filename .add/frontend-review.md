# Frontend Review

> ADD Execution — Frontend Optimization & Policy Enforcement | **Last Updated**: 2026-03-16

## Trigger

User requests frontend code review, optimization, design token audit, i18n audit, or component refactor.

## Read Before Execution

Read only docs relevant to the changed area.

| Doc | Path | When to read |
|-----|------|--------------|
| Frontend patterns (SSOT) | `docs/llm/policies/patterns-frontend.md` | Always |
| Design system (core) | `docs/llm/frontend/design-system.md` | Always |
| Component patterns | `docs/llm/frontend/design-system-components.md` | Component changes |
| i18n rules | `docs/llm/frontend/design-system-i18n.md` | Any i18n change |
| Chart patterns | `docs/llm/frontend/charts.md` | Chart/analytics changes |
| Design tokens | `web/app/tokens.css` | Token compliance check |
| Token module | `web/lib/design-tokens.ts` | Token compliance check |
| i18n keys (source) | `web/messages/en.json` | i18n key parity check |
| i18n keys (ko) | `web/messages/ko.json` | i18n key parity check |
| i18n keys (ja) | `web/messages/ja.json` | i18n key parity check |

---

## Review Checklist (run all in parallel per file)

### 0. 4-Layer Architecture

Token pipeline: `tokens.css (palette)` → `tokens.css (semantic --theme-*)` → `@theme inline (Tailwind)` → `components`

Component layers:

| Layer | Path | Rule |
|-------|------|------|
| 1. Pages | `app/*/page.tsx` | Route entry, `useQuery` wiring only |
| 2. Feature components | `app/*/components/` | Page-specific composed UI — not shared |
| 3. Shared components | `components/` + `components/ui/` | Reusable across pages — no business logic |
| 4. Foundation | `lib/` · `hooks/` · `lib/queries/` | Types, API, formatters, tokens, query factories |

Violations: placing shared logic in feature dirs, or page-specific logic in `components/`.

### 1. Design Token Compliance

| Check | Rule |
|-------|------|
| Inline `style={{}}` colors | Must use `tokens.*` from `@/lib/design-tokens` — never raw `'var(--theme-*)'` strings |
| Tailwind color classes | Only `@theme inline`-generated utilities (e.g. `bg-status-success`, `text-status-warning-fg`) |
| Hardcoded hex | Zero tolerance — no `#xxxxxx` in `.tsx`/`.ts` |
| Gray/slate/zinc bypass | `gray-*` / `slate-*` / `zinc-*` / `stone-*` Tailwind colors forbidden — use semantic tokens |
| Token name accuracy | `status-warning` / `status-warning-fg` (NOT `status-warn` / `status-warn-fg`) |
| SVG fill/stroke | Use `{tokens.*}` expression syntax, not string `fill="var(--theme-*)"` |
| Chart gradients | `stopColor={tokens.*}` (JSX expression), not `stopColor="var(--theme-*)"` |
| Shared color maps | `PROVIDER_COLORS`, `JOB_STATUS_COLORS`, `FINISH_COLORS` in `constants.ts` already use tokens — never duplicate inline |
| 3rd-party wrapper exception | `redoc-wrapper.tsx` and `swagger-ui-wrapper.tsx` use raw hex / `var(--theme-*)` — **legitimate exception**: these libraries accept config objects or `<style>` injection, not JSX. Do not flag these. |

### 2. i18n Compliance

| Check | Rule |
|-------|------|
| User-visible strings | Every string shown in UI must be `t('key')` — no hardcoded English/Korean/Japanese |
| Key parity | All keys in `en.json` must exist in `ko.json` and `ja.json` |
| Formatter usage | Use `fmtMs`, `fmtCompact`, `fmtPct` etc from `chart-theme.ts` — never local `toFixed`/`toLocaleString` for display |
| Missing keys | Add missing keys to all three locale files simultaneously |
| Scope | `t('namespace.key')` — always namespaced, never top-level single word keys |

### 3. Performance

| Check | Rule |
|-------|------|
| Derived state | Wrap filter/sort/map chains from query data in `useMemo` |
| Event handlers in JSX | Stable references via `useCallback` when passed to child components |
| Heavy components | Conditionally rendered heavy panels (modals, charts) → `dynamic(() => import(...), { ssr: false })` |
| Query duplication | Same `queryKey` fetched independently in sibling components → lift to parent or use shared `queryOptions` factory |
| Polling no-ops | Intervals that call `setState` unconditionally → add change-detection guard |
| React key | All list renders must have stable `key` — never `index` as sole key for reorderable lists |
| SSE-driven props | Components receiving props updated ≥1/sec from SSE must be wrapped with `React.memo` — without it every stats tick re-renders the full SVG/DOM tree |
| Time-display staleness | Any component that shows relative time strings (e.g. "5s ago") must have a `setInterval` tick (10–30s) so labels age without waiting for new events |
| Zero-value stat containers | Stat rows/badges that show counts from live data must be hidden when all values are 0 — never show "0 pending, 0 running" noise at idle |

### 4. Component Architecture

| Check | Rule |
|-------|------|
| Chart styles | All Recharts `contentStyle`/`labelStyle`/`itemStyle`/`cursor` must use SSOT constants from `chart-theme.ts` |
| Style maps | All status/role/provider badge class mappings must come from `constants.ts` — never inline duplicates |
| UI logic separation | No API calls / business logic inside `ui/` primitive components |
| Prop count | Handler/page components with >6 props → consider splitting or using a data-prop object |
| Shared pattern 3+ | Same JSX pattern repeated 3+ times → extract shared component |
| Date/time utilities | Before writing a new time formatter, check `lib/date.ts` — `fmtDatetime`, `fmtDatetimeShort`, `fmtDateOnly`, `fmtTimeAgo` already cover most cases |
| Local helper duplication | Module-private helpers (`function foo()` inside a component file) that could be reused → move to the nearest `lib/` utility file |
| StatusPill usage | `components/status-pill.tsx` is the shared component — use it for count+label+icon patterns. `app/providers/components/shared.tsx` has a local `StatusPill` that predates the shared one; do not create more local duplicates |

### 5. TypeScript Strictness

| Check | Rule |
|-------|------|
| `any` usage | Flag all `any` — replace with proper type or `unknown` + guard |
| Non-null assertion `!` | Replace with optional chaining or explicit null check where possible |
| Generated types | Use types from `web/lib/generated/` — never redefine domain enums locally |
| Zod at boundaries | API responses parsed at API layer (`lib/api.ts`) — components receive typed data |

### 6. Accessibility — WCAG 2.1 AA (Admin Dashboard Scope)

**Applicable criteria only** (this is an internal admin tool — media captions 1.2.x not applicable):

| Criterion | Check |
|-----------|-------|
| 1.4.1 Use of Color | Status conveyed by color MUST also have icon or text (e.g. status dot + icon + label) |
| 1.4.3 Contrast | Minimum 4.5:1 for normal text — design tokens already exceed AA; flag any hardcoded low-contrast colors |
| 2.1.1 Keyboard | All interactive elements reachable by Tab; dialogs trap focus |
| 2.4.7 Focus Visible | All focusable elements have `focus-visible:` ring — use `--theme-focus-ring` token |
| 4.1.2 Name/Role/Value | Icon-only buttons must have `aria-label`; form inputs must have `<Label>` or `aria-label` |
| Loading states | Spinner/skeleton must have `aria-label="Loading"` or `aria-busy` |

**Not applicable:**
- 1.2.x (Audio/Video captions) — app has no media content
- 1.4.4 Resize text — browser-native behavior, no override
- 2.4.5 Multiple ways — single-page admin app, N/A

---

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Get the diff: `git diff HEAD` (or read user-specified files) |
| 2 | Launch 3 parallel review agents (Reuse · Quality · Efficiency) — pass full diff to each |
| 3 | Aggregate findings; discard false positives with reason |
| 4 | Fix violations in order of severity (P0 → P1 → P2); each fix = one logical round |
| 5 | For i18n: update `en.json`, `ko.json`, `ja.json` simultaneously in one edit |
| 6 | Run `npx tsc --noEmit` — zero errors required before continuing |
| 7 | If user requests N rounds: repeat steps 2–6 until N rounds consumed or no violations remain |
| 8 | CDD sync — update the relevant doc(s) if a new pattern is established: |

**Step 8 — which doc to update:**

| What changed | Update target |
|--------------|---------------|
| New token usage pattern | `docs/llm/policies/patterns-frontend.md` + `docs/llm/frontend/design-system.md` |
| New component pattern or shared component | `docs/llm/frontend/design-system-components.md` |
| New i18n rule or key convention | `docs/llm/frontend/design-system-i18n.md` |
| New chart/formatter pattern | `docs/llm/frontend/charts.md` |
| New review rule (performance, perf, a11y) | `.add/frontend-review.md` (this file) |
| New query/data-fetch pattern | `docs/llm/policies/patterns-frontend.md` |

## Fix Iteration Policy

When running **N rounds of fixes** (e.g. "10회 수정"):

- Each *round* = one logical fix (a single coherent change, not a file save)
- After every 3–4 rounds, run `tsc --noEmit` to catch regressions early
- False positives count as a round (document why the finding was skipped)
- Stop early if no remaining violations — do not manufacture changes to hit the count
- Parallel review agents always run **before** fixes begin, not interleaved

## Fix Priority

| Priority | Category |
|----------|----------|
| P0 (fix immediately) | Hardcoded hex, wrong token names, broken i18n keys, missing i18n parity across locales |
| P1 (fix in same pass) | Raw `var(--theme-*)` strings, missing `useMemo`, missing `aria-label`, SSE-driven components without `React.memo`, time-display staleness (no interval tick) |
| P2 (fix if touching file) | Component extraction for 3+ duplicates, prop count reduction, zero-value stat containers, local helpers duplicating `lib/` utilities |
