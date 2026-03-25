# Code Patterns: Frontend — 2026 Reference

> SSOT | **Last Updated**: 2026-03-18 | Classification: Operational | Exception: >200 lines (pattern registry)
> Next.js 16 · React 19 · TanStack Query v5 · Tailwind v4 · Zod
> Rust patterns -> `policies/patterns.md`

---

## TanStack Query v5

### `queryOptions()` Factory -- SSOT Pattern

Define query config once in `web/lib/queries/`, reuse across components:

```typescript
// web/lib/queries/dashboard.ts
import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST, REFETCH_INTERVAL_FAST } from '@/lib/constants'

export const dashboardStatsQuery = queryOptions({
  queryKey: ['dashboard', 'stats'],
  queryFn: () => api.stats(),
  staleTime: STALE_TIME_FAST,
  retry: false,
})
```

```typescript
// In a page component
const { data } = useQuery(dashboardStatsQuery)
```

Benefits: single place to change staleTime/retry, type-safe key sharing, reuse in `prefetchQuery`.

### Query Timing Constants

All `staleTime` and `refetchInterval` values come from `web/lib/constants.ts`:

| Constant | Value | Used by |
|----------|-------|---------|
| `STALE_TIME_SLOW` | 59s | keys, usage, accounts, audit, servers |
| `STALE_TIME_FAST` | 29s | dashboard stats, capacity, providers |
| `STALE_TIME_HISTORY` | 30min | long-window historical queries (metrics history) |
| `REFETCH_INTERVAL_FAST` | 30s | dashboard stats, capacity, providers |

Never hardcode timing values in query definitions — import from constants.

### Query Key Constants — Invalidation SSOT

For groups of related queries (e.g. Gemini), export key constants alongside `queryOptions`:

```typescript
// web/lib/queries/providers.ts
export const GEMINI_QUERY_KEYS = {
  syncConfig:     ['gemini-sync-config'] as const,
  models:         ['gemini-models'] as const,
  policies:       ['gemini-policies'] as const,
  selectedModels: ['selected-models'] as const,
} as const
```

Page components import and use these for invalidation — never duplicate key arrays inline.

### Inline `useQuery` (one-off, modal-only fetches)

```typescript
const { data } = useQuery({
  queryKey: ['job-detail', jobId],
  queryFn: () => api.jobDetail(jobId!),
  enabled: !!jobId && open,
})
```

### Mutation -- `onSettled` for cache invalidation

```typescript
// CORRECT -- onSettled runs on both success and error
const mutation = useMutation({
  mutationFn: (id: string) => api.deleteProvider(id),
  onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  onError: (e: Error) => console.error(e.message),
})
mutation.mutate(id)            // fire-and-forget
await mutation.mutateAsync(id) // await inside async handler

// WRONG -- onSuccess skips invalidation on error (stale UI)
onSuccess: () => queryClient.invalidateQueries(...)
```

---

## React 19 -- useOptimistic

Apply optimistic updates to all toggle/switch mutations for perceived speed.

```typescript
import { useOptimistic } from 'react'

const [optimisticEnabled, setOptimistic] = useOptimistic(
  model.is_enabled,
  (_, newValue: boolean) => newValue
)

const mutation = useMutation({
  mutationFn: (v: boolean) => api.setModelEnabled(providerId, model.model_name, v),
  onError: () => setOptimistic(model.is_enabled),
})

<Switch
  checked={optimisticEnabled}
  onCheckedChange={(v) => { setOptimistic(v); mutation.mutate(v) }}
/>
// UI responds instantly -> server syncs in background -> reverts if error
```

---

## TypeScript + Zod (API Boundary Validation)

TypeScript enforces compile-time types; Zod validates untrusted API responses at runtime.

```typescript
// web/lib/types.ts
import { z } from 'zod'

export const ProviderSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  provider_type: z.enum(['ollama', 'gemini']),
  status: z.enum(['online', 'offline', 'degraded']),
  is_active: z.boolean(),
})
export type Provider = z.infer<typeof ProviderSchema>

// safeParse for graceful error handling (no throws)
const result = ProviderSchema.safeParse(apiResponse)
if (!result.success) console.error(result.error.issues)

// Branded types prevent wrong-ID bugs
const ProviderIdSchema = z.string().uuid().brand<'ProviderId'>()
type ProviderId = z.infer<typeof ProviderIdSchema>
```

Apply Zod at entry points: API responses, form inputs, env vars.

### FlowStats — Server-Computed Rates

`FlowStatsSchema` fields: `incoming` (10s window count), `incoming_60s` (60s window count = req/m), `queued`, `running`, `completed`. All `NonNegativeInt`.

- `req/s` = `incoming / 10` (client divides)
- `req/m` = `incoming_60s` (server-computed 60-bucket sliding window, NOT `req/s * 60`)
- Server broadcasts every second unconditionally — clients rely on this cadence

---

## Shared Style Constants

All Tailwind class mappings live in `web/lib/constants.ts` — never duplicate across components:

| Constant | Keys | Purpose |
|----------|------|---------|
| `STATUS_STYLES` | completed, failed, cancelled, pending, running | Job status badge classes |
| `ROLE_STYLES` | system, user, assistant, tool | Chat message role badge classes |
| `FINISH_BG` | stop, length, error, cancelled | Finish reason badge classes |
| `FINISH_COLORS` | stop, length, error, cancelled | Finish reason chart colours |
| `PROVIDER_BADGE` | ollama, gemini | Provider type badge classes |
| `PROVIDER_COLORS` | ollama, gemini | Provider type chart colours |

Import from `@/lib/constants` — never duplicate style mappings across components.

## Chart Tooltip Style

`TOOLTIP_STYLE` in `web/lib/chart-theme.ts` is the SSOT for Recharts tooltip `contentStyle`:

```typescript
export const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '0.5rem',
  color: 'var(--theme-text-primary)',
}
```

Import and spread into `<Tooltip contentStyle={TOOLTIP_STYLE} />` — never inline tooltip styles.

## Chart Theme Formatters

All display formatters live in `web/lib/chart-theme.ts` — never define local formatting functions:

| Function | Example | Usage |
|----------|---------|-------|
| `fmtCompact(n)` | 1500 → "1.5K" | KPI cards, chart labels |
| `fmtMs(n)` | 1400 → "1.4s" | Latency display |
| `fmtMsNullable(n)` | null → "—" | Nullable latency |
| `fmtMsAxis(n)` | 86360 → "1.4m" | Chart Y-axis ticks |
| `fmtPct(n)` | 0.956 → "96%" | Success rates |
| `fmtMbShort(mb)` | 2048 → "2.0 GB" | VRAM display |

## useMemo for Derived Data

Wrap filter/sort/slice/map chains with `useMemo` when the input comes from query data:

```typescript
const modelBarData = useMemo(() =>
  (breakdown?.by_model ?? [])
    .filter(m => geminiEnabled || m.provider_type !== 'gemini')
    .sort((a, b) => b.request_count - a.request_count)
    .slice(0, 8)
    .map(m => ({ ...m, label: truncate(m.model_name, 22) })),
  [breakdown?.by_model, geminiEnabled],
)
```

Not needed for simple property access or single-value derivations.

## Design Token System (4-Layer Architecture)

Token flow: `tokens.css palette` → `tokens.css semantic (--theme-*)` → `@theme inline (Tailwind utilities)` → components

### Layer usage by context

| Context | Correct pattern | Wrong |
|---------|----------------|-------|
| Tailwind className | `bg-status-success`, `text-status-warning-fg` | `text-emerald-400`, `bg-green-600` |
| Inline `style={{}}` | `import { tokens } from '@/lib/design-tokens'` → `tokens.status.success` | `'var(--theme-status-success)'` raw string |
| SVG fill/stroke | `fill={tokens.status.info}` (JSX expression) | `fill="var(--theme-status-info)"` string |
| Chart gradient stopColor | `stopColor={tokens.brand.primary}` | `stopColor="var(--theme-primary)"` |
| Recharts fill/stroke | `fill={tokens.status.success}` | `fill="var(--theme-status-success)"` |

### `tokens` module structure (`web/lib/design-tokens.ts`)

```typescript
import { tokens } from '@/lib/design-tokens'

// Backgrounds
tokens.bg.page | .card | .elevated | .hover

// Text
tokens.text.primary | .secondary | .bright | .dim | .faint

// Border
tokens.border.base | .subtle | .default

// Brand
tokens.brand.primary | .foreground | .ring | .focusRing

// Status (background / icon colour)
tokens.status.success | .error | .warning | .info | .cancelled

// Status foreground (text colour)
tokens.statusFg.success | .error | .warning | .info

// Accents
tokens.accent.gpu | .power | .brand

// Charts
tokens.chart.c1 | .c2 | .c3 | .c4 | .c5
```

### Token name rules

```
status-warning     ✓    status-warn     ✗
status-warning-fg  ✓    status-warn-fg  ✗
```

### Correct full example

```tsx
import { tokens } from '@/lib/design-tokens'

// Inline style — always tokens module
<span style={{ background: tokens.status.success }} />

// className — always Tailwind semantic utilities
<span className="bg-status-success text-status-success-fg" />

// SVG / Recharts
<Bar fill={tokens.status.info} />
<stop stopColor={tokens.brand.primary} stopOpacity={0.35} />

// Never
<span style={{ color: 'var(--theme-text-secondary)' }} />   // ✗ raw string
<span className="text-emerald-400" />                        // ✗ bypasses theme
<span style={{ color: '#065f46' }} />                        // ✗ hardcoded hex
```

---

## E2E Test Patterns

### Test Constants

All E2E test constants live in `web/e2e/helpers/constants.ts`:

| Export | Purpose |
|--------|---------|
| `API_BASE_URL` | Backend URL (env: `PLAYWRIGHT_API_URL`, default: `localhost:3001`) |
| `TEST_USERNAME` | Login username (env: `E2E_USERNAME`, default: `admin`) |
| `TEST_PASSWORD` | Login password (env: `E2E_PASSWORD`, default: `changeme`) |
| `testId()` | Generate unique 8-char suffix for test resources |

### Resource Cleanup

CRUD lifecycle tests use `try/finally` to clean up created resources:

```typescript
let createdId: string | undefined
try {
  const res = await api.post('/v1/keys', { name: `e2e-${testId()}` })
  createdId = (await res.json()).id
  // ... assertions ...
} finally {
  if (createdId) await api.delete(`/v1/keys/${createdId}`)
}
```

---

## UI-State Types in `web/lib/types.ts`

Modal/form state types that appear across multiple components belong in `web/lib/types.ts`, not as local `type` definitions.

```typescript
// web/lib/types.ts
export type VerifyState = 'idle' | 'checking' | 'ok' | 'error'
```

Import in components: `import type { VerifyState } from '@/lib/types'`

Rule: if the same `type Foo = 'a' | 'b' | ...` appears in 2+ component files, move it to `lib/types.ts`.

---

## HTTP Errors with Status Code (`ApiHttpError`)

Custom fetch helpers that need to distinguish HTTP status codes throw `ApiHttpError` from `web/lib/types.ts`:

```typescript
// lib/api.ts — throwing
import { ApiHttpError } from './types'
if (!res.ok) throw new ApiHttpError(data.error ?? `${res.status}`, res.status)

// Component onError — handling
import { ApiHttpError } from '@/lib/types'
onError: (e) => {
  const msg = e instanceof ApiHttpError && e.status === 409
    ? t('...duplicateUrl')
    : (e instanceof Error ? e.message : t('...connectionFailed'))
}
```

Rule: never cast `(e as Error & { status?: number })` — use `instanceof ApiHttpError` instead.

---

## SVG Pattern IDs — `useId()` for DOM Uniqueness

SVG `<pattern id="...">` elements use global DOM IDs. If a component can render multiple instances, use `React.useId()` to generate unique IDs. Strip `:` from the result — React IDs like `:r1:` are not valid XML NCNames.

```tsx
import { useId } from 'react'

const rawId = useId()
const safeId = rawId.replace(/:/g, '') // React IDs contain ':' which is invalid in SVG NCNames
const patternId = `my-pattern-${safeId}`

// In SVG:
<pattern id={patternId} .../>
<rect fill={`url(#${patternId})`} />
```

---

## Query Prefetch in AppShell

Queries depended on by multiple pages (e.g. `serversQuery` drives the dashboard waterfall) should be prefetched in `AppShell` on mount so they are cache-warm before the user navigates.

```tsx
// web/app/layout.tsx — AppShell
const queryClient = useQueryClient()
useEffect(() => {
  if (!isLoginPage && !isSetupPage && isLoggedIn()) {
    queryClient.prefetchQuery(serversQuery)
  }
  // isLoggedIn() is a pure synchronous cookie read — not React state, omit from deps
  // eslint-disable-next-line react-hooks/exhaustive-deps
}, [queryClient, isLoginPage, isSetupPage])
```

**Rule**: only prefetch queries that are universally needed across authenticated pages. Page-specific queries stay in the page component.

## Historical Data — `STALE_TIME_HISTORY`

Long-window historical queries (e.g. 60-day power/metrics history) use `STALE_TIME_HISTORY` (30 minutes).
Background refetch still runs on `REFETCH_INTERVAL_HISTORY` (5 minutes) to keep data fresh,
but re-navigation within 30 minutes skips the on-mount fetch and returns cached data immediately.

```typescript
// web/lib/queries/servers.ts
export const serverMetricsHistoryQuery = (serverId: string) => queryOptions({
  queryKey: ['server-metrics-history', serverId],
  queryFn: () => api.serverMetricsHistory(serverId),
  staleTime: STALE_TIME_HISTORY,           // 30 min — re-nav returns cache instantly
  refetchInterval: REFETCH_INTERVAL_HISTORY, // 5 min — background refresh continues
})
```

**Rule**: `staleTime` and `refetchInterval` should reflect how quickly data actually changes, not be set to "slightly less than refetch". Use `STALE_TIME_HISTORY` for any query whose data window spans days or weeks.

---

## Page Guard (`usePageGuard`)

Menu-based access control at page level. Redirects to `/overview` if user lacks the required menu permission. Super-admin bypasses all checks.

```typescript
// web/hooks/use-page-guard.ts
export function usePageGuard(menuId: string): void
// Usage: usePageGuard('audit') at top of page component
```

`hasMenu()` reads from JWT claims `menus` array (set during login from merged role menus).

---

## Adding a New Page

```
1. web/lib/types.ts            -- add TypeScript types (+ Zod schema if untrusted data)
2. web/lib/api.ts              -- add API functions to the api object
3. web/lib/queries/domain.ts   -- add queryOptions factory (SSOT for queryKey + staleTime)
4. web/app/new-page/page.tsx   -- 'use client' + useQuery(domainQuery) + UI
5. web/components/nav.tsx      -- add navItems entry
6. web/messages/en.json        -- add i18n keys (source of truth)
7. web/messages/ko.json        -- Korean translation
8. web/messages/ja.json        -- Japanese translation
9. docs/llm/frontend/pages/*   -- update CDD doc
```

---

## 4-Layer Component Architecture

| Layer | Path | Rule |
|-------|------|------|
| 1. Pages | `app/*/page.tsx` | Route entry — `useQuery` wiring + layout only |
| 2. Feature components | `app/*/components/` | Page-specific composed UI — not shared |
| 3. Shared components | `components/` + `components/ui/` | Reusable across pages — no business logic |
| 4. Foundation | `lib/` · `hooks/` · `lib/queries/` | Types, API, formatters, tokens, query factories |

Violations: shared logic in feature dirs, or page-specific logic in `components/`.

---

## i18n Compliance

| Rule | Detail |
|------|--------|
| All UI strings | Must use `t('namespace.key')` — no hardcoded English/Korean/Japanese |
| Key parity | All keys in `en.json` must exist in `ko.json` and `ja.json` |
| Formatter usage | Use `fmtMs`, `fmtCompact`, `fmtPct` etc from `chart-theme.ts` — never local `toFixed`/`toLocaleString` for display |
| Missing keys | Add to all three locale files simultaneously |
| Namespace | Always `t('namespace.key')` — never top-level single-word keys |

---

## Performance Rules

| Rule | Detail |
|------|--------|
| Derived state | Wrap filter/sort/map chains from query data in `useMemo` |
| Handler refs | Stable references via `useCallback` when passed to child components |
| Heavy panels | Modals/charts with conditional render → `dynamic(() => import(...), { ssr: false })` |
| Query dedup | Same `queryKey` in sibling components → lift to parent or share `queryOptions` factory |
| SSE-driven components | Props updated ≥1/s from SSE or `setInterval` ≤100ms → `React.memo` required |
| Time-display staleness | Components showing relative time (e.g. "5s ago") → `setInterval` tick (10–30s) required |
| Zero-value stat containers | Stat rows showing counts from live data → hidden when all values are 0 |
| React key | Never use array `index` as sole key for reorderable lists |

---

## TypeScript Strictness

| Rule | Detail |
|------|--------|
| No `any` | Replace with proper type or `unknown` + type guard |
| Non-null `!` | Replace with optional chaining or explicit null check where possible |
| Generated types | Use types from `web/lib/generated/` — never redefine domain enums locally |
| Zod at boundaries | Parse API responses at `lib/api.ts` — components receive typed data |
| UI state types | `type Foo = 'a' \| 'b' \| ...` shared across 2+ files → move to `lib/types.ts` |

---

## Accessibility — WCAG 2.1 AA (Admin Dashboard Scope)

| Criterion | Check |
|-----------|-------|
| 1.4.1 Use of Color | Status conveyed by color MUST also have icon or text |
| 1.4.3 Contrast | Min 4.5:1 for normal text — design tokens already exceed AA; flag hardcoded low-contrast |
| 2.1.1 Keyboard | All interactive elements reachable by Tab; dialogs trap focus |
| 2.4.7 Focus Visible | All focusable elements have `focus-visible:` ring — use `--theme-focus-ring` token |
| 4.1.2 Name/Role/Value | Icon-only buttons → `aria-label`; form inputs → `<Label>` or `aria-label` |
| Loading states | Spinner/skeleton → `aria-label="Loading"` or `aria-busy` |

Not applicable: 1.2.x (no media), 1.4.4 resize (browser-native), 2.4.5 multiple ways (single-page admin).

---

## Review Fix Priority

| Priority | Category |
|----------|----------|
| P0 (fix immediately) | Hardcoded hex, wrong token names, broken i18n keys, missing i18n parity |
| P1 (fix in same pass) | Raw `var(--theme-*)` strings, missing `useMemo`, missing `aria-label`, SSE components without `React.memo`, time-display without interval tick |
| P2 (fix if touching file) | Component extraction for 3+ duplicates, prop count reduction, zero-value stat containers |
