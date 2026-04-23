# Frontend Review

> ADD Execution — Frontend Optimization & Policy Enforcement | **Last Updated**: 2026-04-22

## Trigger

User requests frontend code review, optimization, design token audit, i18n audit, or component refactor.

## Read Before Execution

| Doc | Path | When |
|-----|------|------|
| Execution contracts | `docs/llm/frontend/execution-contracts.md` | Always — folder structure, state classification, naming |
| Frontend patterns (SSOT) | `docs/llm/policies/patterns-frontend.md` | Always — contains all checklists |
| Design system (core) | `docs/llm/frontend/design-system.md` | Always |
| Component patterns | `docs/llm/frontend/design-system-components.md` | Component changes |
| Component patterns (extended) | `docs/llm/frontend/design-system-components-patterns.md` | ConfirmDialog, useApiMutation, 2-Step Verify, nav-404, Accounts |
| i18n rules | `docs/llm/frontend/design-system-i18n.md` | i18n changes |
| Chart patterns | `docs/llm/frontend/charts.md` | Chart/analytics changes |
| Design tokens | `web/app/tokens.css` + `web/lib/design-tokens.ts` | Token compliance |
| i18n sources | `web/messages/en.json` + `ko.json` + `ja.json` | Key parity check |
| Page doc | `docs/llm/frontend/pages/{page}.md` | When reviewing a specific page — read its page doc if it exists |

> Checklist details (4-layer arch, design tokens, i18n, performance, TypeScript, a11y, fix priority) → `docs/llm/policies/patterns-frontend.md`

## Architecture Non-Goals (reject on sight)

Do NOT propose or accept any of the following during review:

- New directories named `atoms/`, `molecules/`, `organisms/`, or `templates/`
- Atomic Design vocabulary in file names, component names, or review comments
- Renaming 4-Layer terms to Atomic equivalents
- Hardcoded hex / Tailwind raw color scales (`gray-*`/`emerald-*`/`zinc-*`/`bg-[#123]`) in `.tsx`
- Color definitions anywhere outside `web/app/tokens.css` (including inline `var(--theme-*)` strings)
- Single dark-mode selector (`.dark` only or `[data-theme='dark']` only) — must be both

Rationale: `patterns-frontend/architecture.md § 4-Layer Component Architecture / Non-Goals` and `§ Design Token System / Single Source of Truth`.

---

## Execution Steps

| Step | Action |
|------|--------|
| 1 | Get the diff: `git diff HEAD` or read user-specified files |
| 2 | Launch 3 parallel review agents (Reuse · Quality · Efficiency) — pass full diff + agent scope below to each |
| 3 | Aggregate findings; discard false positives with reason |
| 4 | Fix violations P0 → P1 → P2 (see fix priority in `patterns-frontend.md`) |
| 5 | For i18n: update `en.json`, `ko.json`, `ja.json` simultaneously |
| 6 | Run `npx tsc --noEmit` — zero errors required |
| 7 | If N rounds requested: repeat steps 2–6 until N rounds consumed or no violations remain |
| 8 | CDD feedback — run `.add/cdd-feedback.md` if a new pattern is confirmed (target doc table below) |

### Agent Scope

**Reuse agent** — checks that existing abstractions are used instead of reinvented:
- Single-importer shared component: any file in `web/components/` imported by exactly one route → flag for move to `app/{route}/components/` (→ `patterns-frontend.md` § 4-Layer Component Architecture / Violations)
- Color defined outside `tokens.css`: any new `--*-color` variable or hex value anywhere except `web/app/tokens.css` → reject (→ `patterns-frontend.md` § Design Token System / Single Source of Truth)
- `DataTable` used for all tables (never raw Card+Table boilerplate) (→ `design-system.md` § Task Guide)
- `ConfirmDialog` for destructive actions (never `confirm()` native dialog) (→ `design-system-components-patterns.md` § ConfirmDialog)
- `CopyButton`, `StatusPill`, `StatsCard`, `ProgressBar`, `TimeRangeSelector` — check for hand-rolled equivalents (→ `design-system-components.md`)
- `useApiMutation` for mutations needing query invalidation (no repeated `useQueryClient()` + `onSettled` boilerplate) (→ `design-system-components-patterns.md` § useApiMutation)
- `fmtMs`, `fmtCompact`, `fmtPct`, `fmtMbShort`, `fmtMsAxis` from `chart-theme.ts` — no local `toFixed`/`toLocaleString` for display (→ `patterns-frontend.md` § Chart Theme Formatters)
- `TOOLTIP_STYLE` from `chart-theme.ts` — never inline tooltip `contentStyle` (→ `patterns-frontend.md` § Chart Tooltip Style)
- `STATUS_STYLES`, `PROVIDER_BADGE`, `PROVIDER_COLORS`, `FINISH_COLORS` from `constants.ts` — no duplicate style maps (→ `patterns-frontend.md` § Shared Style Constants)
- `queryOptions()` factory in `web/lib/queries/` — no inline `useQuery({queryKey, queryFn})` for queries used in 2+ places (→ `patterns-frontend.md` § TanStack Query v5 / `queryOptions()` Factory)
- Query timing constants (`STALE_TIME_FAST/SLOW/HISTORY`, `REFETCH_INTERVAL_FAST`, `withJitter()`) — never hardcode `30_000` or similar values (→ `patterns-frontend.md` § TanStack Query v5 / Query Timing Constants)
- Query key constants (`GEMINI_QUERY_KEYS` pattern) for groups of related queries (→ `patterns-frontend.md` § TanStack Query v5 / Query Key Constants)

**Quality agent** — checks correctness and pattern compliance:
- No raw `fetch()` in components — all HTTP via `apiGet`/`apiPost`/`apiFetch` from `lib/api.ts` (→ `execution-contracts.md` § Common Module Import Contract)
- No raw `setInterval` in components — polling via `usePolling` from `lib/stream.ts` (→ `execution-contracts.md` § Realtime Contract)
- Feature components in `app/{route}/components/` only — no cross-route imports (→ `execution-contracts.md` § Feature Boundary Rules)
- `onSettled` (not `onSuccess`) for mutation cache invalidation (→ `patterns-frontend.md` § TanStack Query v5 / Mutation -- onSettled)
- `useOptimistic` on all toggle/switch mutations (→ `patterns-frontend.md` § React 19 -- useOptimistic)
- `ApiHttpError instanceof` checks — never `(e as any).status` or type casts (→ `patterns-frontend.md` § HTTP Errors with Status Code)
- `usePageGuard(menuId)` present on new pages (→ `patterns-frontend.md` § Page Guard)
- 2-Step Verify Flow for registration modals: URL change resets verify state, register button gated on `isVerified` and URL hasn't changed (→ `design-system-components-patterns.md` § 2-Step Verify Flow)
- SVG `<pattern id>` uses `useId()` with non-alphanumeric chars stripped — never static strings in multi-instance components (→ `patterns-frontend.md` § SVG Pattern IDs)
- `useMemo` wrapping filter/sort/map chains from query data (→ `patterns-frontend.md` § useMemo for Derived Data)
- `useCallback` on handlers passed to child components (→ `patterns-frontend.md` § Performance Rules)
- `refetchInterval` uses `withJitter(REFETCH_INTERVAL_FAST)` — never bare constant (prevents tab polling storms) (→ `patterns-frontend.md` § TanStack Query v5 / `withJitter()`)
- `PUBLIC_PATHS` updated for any new unauthenticated route (→ `design-system-components.md` § Auth Guard)
- 4-layer architecture: page logic in `app/*/page.tsx`, feature UI in `app/*/components/`, shared in `components/`, foundation in `lib/` (→ `patterns-frontend.md` § 4-Layer Component Architecture)
- Tests are behavior-driven: no `querySelector`, no CSS-class queries, no React-internal assertions, no full-DOM snapshots, no mock-call-count as primary assertion (→ `testing-strategy.md` § Behavior-Driven Tests)
- Testing Library query priority: `getByRole` > `getByLabelText` > `getByText` > `getByTestId` — drop only with comment explaining why (→ `testing-strategy.md` § Testing Library Query Priority)
- Layout / focus / CSS / scroll assertions are NOT in jsdom unit tests — those belong in Vitest Browser Mode component tests (→ `testing-strategy.md` § Layer Responsibility)
- Test layer fits the change: internal function change breaks only Unit; schema change breaks only Integration; user-flow change breaks only E2E (→ `testing-strategy.md` § Test Purity Principle)
- E2E tests: constants from `helpers/constants.ts`, `try/finally` resource cleanup (→ `patterns-frontend.md` § E2E Test Patterns)

**Efficiency agent** — checks rendering and data performance:
- `React.memo` on components receiving props at ≥1/s (SSE-driven, `setInterval` ≤100ms) (→ `patterns-frontend.md` § Performance Rules)
- `dynamic(() => import(...), { ssr: false })` for heavy panels rendered conditionally (→ `patterns-frontend.md` § Performance Rules)
- No duplicate `queryKey` across sibling components (lift or share `queryOptions` factory) (→ `patterns-frontend.md` § TanStack Query v5 / `queryOptions()` Factory)
- Zero-value stat containers hidden when all values are 0
- Relative time displays ("5s ago") have `setInterval` tick (10–30s)
- No array `index` as sole React key for reorderable lists (→ `patterns-frontend.md` § Performance Rules)
- `refetchOnWindowFocus: false` respected — no per-query override without comment (→ `patterns-frontend.md` § TanStack Query v5 / Query Timing Constants)

**Step 8 — which doc to update:**

| What changed | Target |
|--------------|--------|
| Token usage pattern | `docs/llm/policies/patterns-frontend.md` + `docs/llm/frontend/design-system.md` |
| New component pattern | `docs/llm/frontend/design-system-components.md` |
| i18n rule or key convention | `docs/llm/frontend/design-system-i18n.md` |
| Chart/formatter pattern | `docs/llm/frontend/charts.md` |
| Review rule (perf, a11y) | `docs/llm/policies/patterns-frontend.md` |

## Fix Iteration Policy

- Each *round* = one logical fix (a single coherent change)
- After every 3–4 rounds, run `tsc --noEmit` to catch regressions early
- False positives count as a round (document why the finding was skipped)
- Stop early if no remaining violations — do not manufacture changes to hit the count
- Parallel review agents always run **before** fixes begin, not interleaved
