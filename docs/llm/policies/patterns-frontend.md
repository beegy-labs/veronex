# Code Patterns: Frontend — 2026 Reference

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational | Exception: >200 lines (pattern registry)
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
| `REFETCH_INTERVAL_HISTORY` | 5min | background refresh for historical data |

Never hardcode timing values in query definitions — import from constants.

### `withJitter()` — Polling Storm Prevention

Use `withJitter()` on every `refetchInterval` to prevent synchronized polling bursts when many browser tabs open simultaneously:

```typescript
import { REFETCH_INTERVAL_FAST, withJitter } from '@/lib/constants'

export const mcpServersQuery = () => queryOptions({
  queryKey: ['mcp-servers'],
  queryFn: () => api.mcpServers(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST), // ✓ jittered
  refetchIntervalInBackground: false,
})

// Wrong — all tabs fire at exactly the same time
refetchInterval: REFETCH_INTERVAL_FAST  // ✗
```

`withJitter(base, maxJitter=5_000)` returns `base + U[0, maxJitter)` ms — always ≥ base.

### `queryOptions()` — Object vs Factory Function

Use a **plain object** when the query has no dynamic parameters:

```typescript
// Plain object — use when queryKey has no variables
export const dashboardStatsQuery = queryOptions({
  queryKey: ['dashboard', 'stats'],
  queryFn: () => api.stats(),
  staleTime: STALE_TIME_FAST,
})

// Usage: useQuery(dashboardStatsQuery)  — no call parens
```

Use a **factory function** when the query depends on a parameter:

```typescript
// Factory function — use when queryKey contains a variable
export const mcpServersQuery = () => queryOptions({
  queryKey: ['mcp-servers'],
  queryFn: () => api.mcpServers(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
})

export const serverMetricsHistoryQuery = (serverId: string) => queryOptions({
  queryKey: ['server-metrics-history', serverId],
  queryFn: () => api.serverMetricsHistory(serverId),
  staleTime: STALE_TIME_HISTORY,
})

// Usage: useQuery(mcpServersQuery())  — with call parens
```

| Case | Form | Reason |
|------|------|--------|
| Static queryKey, no `refetchInterval` callback | Plain object | Simpler call site |
| queryKey contains a variable | Factory function | Key must vary per argument |
| `refetchInterval: () => withJitter(...)` | Factory function | Callback form requires factory; `withJitter` MUST be a callback to prevent polling storms |

Rule: when `refetchInterval` uses a callback (`() => withJitter(...)`), the query MUST be a factory function — plain objects cannot hold function-valued `refetchInterval` without the factory wrapper.

### `mutationOptions()` Factory (v5.82+)

The mutation equivalent of `queryOptions()`. Define mutation config once and reuse with `useMutation`, `useIsMutating`, and `queryClient.isMutating`:

```typescript
// web/lib/queries/mcp.ts
import { mutationOptions } from '@tanstack/react-query'

export const registerMcpServerMutation = mutationOptions({
  mutationKey: ['mcp-register'],
  mutationFn: (body: RegisterMcpServerRequest) => api.registerMcpServer(body),
})

// Usage
const mutation = useMutation(registerMcpServerMutation)
```

Use when the same mutation is referenced from multiple components or when you need typed `mutationKey` for `useIsMutating`.

### `useSuspenseQuery` — Data-Guaranteed Rendering

Prefer `useSuspenseQuery` over `useQuery` when the component always needs data to render. Eliminates `data | undefined` type overhead — `data` is always `T`.

```typescript
// ✓ useSuspenseQuery — data is T, no undefined check needed
const { data } = useSuspenseQuery(dashboardStatsQuery)
return <Chart data={data} />

// ✗ useQuery — data is T | undefined, requires null check
const { data } = useQuery(dashboardStatsQuery)
if (!data) return null
return <Chart data={data} />
```

Wrap the page or component with `<Suspense fallback={<Loading />}>`. `useSuspenseQuery` does not accept `enabled` — use `skipToken` instead for conditional queries.

### `skipToken` — Conditional Queries (TypeScript-idiomatic)

Use `skipToken` instead of `enabled: false` when the query depends on a value that may be undefined:

```typescript
import { skipToken } from '@tanstack/react-query'

const { data } = useQuery({
  queryKey: ['job-detail', jobId],
  queryFn: jobId ? () => api.jobDetail(jobId) : skipToken,
})
```

Rule: `enabled: false` is still valid for boolean flags (e.g. `enabled: !!jobId && open`). Use `skipToken` when the `queryFn` itself would be invalid to call (no valid arguments).

### `useMutationState` — Cross-Component Mutation Observation

Read in-flight or completed mutation state from the global `MutationCache` without prop drilling:

```typescript
import { useMutationState } from '@tanstack/react-query'

// Show a global loading indicator for any pending key registration
const pendingKeyNames = useMutationState({
  filters: { mutationKey: ['key-register'], status: 'pending' },
  select: (mutation) => mutation.state.variables as string,
})
```

### `experimental_streamedQuery` — SSE Streaming Queries

For SSE or `AsyncIterable`-returning endpoints (LLM streaming, real-time feeds):

```typescript
import { experimental_streamedQuery } from '@tanstack/react-query'

useQuery({
  queryKey: ['chat', threadId],
  queryFn: experimental_streamedQuery({
    queryFn: ({ signal }) => api.streamChat(threadId, signal),
    refetchMode: 'reset',   // 'append' | 'reset' | 'replace'
    maxChunks: 100,
  }),
})
```

Query enters `success` after the first chunk; data is an array of all received chunks. Currently prefixed `experimental_` — do not use in stable production paths.

### `isEnabled` Return Value (v5.83+)

`useQuery` now returns `isEnabled` — use it instead of recomputing the enabled condition in render:

```typescript
const { data, isEnabled } = useQuery({
  queryKey: ['lab-settings'],
  queryFn: () => api.labSettings(),
  enabled: featureFlag && isLoggedIn,
})
if (!isEnabled) return null
```

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

`invalidateQueries` MUST be in `onSettled`, never in `onSuccess`. `onSuccess` skips on network error, leaving stale data in the UI until the next refetch cycle.

```typescript
// REQUIRED — onSettled runs on both success and error
const mutation = useMutation({
  mutationFn: (id: string) => api.deleteProvider(id),
  onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  onError: (e: Error) => console.error(e.message),
})
mutation.mutate(id)            // fire-and-forget
await mutation.mutateAsync(id) // await inside async handler

// WRONG — onSuccess skips invalidation on error (stale UI)
onSuccess: () => queryClient.invalidateQueries(...)  // ✗
```

Rule: every `useMutation` that changes server state MUST include `onSettled` with `invalidateQueries` for the affected query key(s).
`onSuccess` may still be used for UI-only side effects (closing a dialog on success, showing a saved indicator) — the restriction applies to `invalidateQueries` only.

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

## React Compiler (v1.0, October 2025)

React Compiler handles `useMemo`, `useCallback`, and `React.memo` automatically for the vast majority of components. **Do not add manual memoization unless one of these specific conditions applies:**

1. Third-party library requires reference equality the compiler can't satisfy (e.g., react-dnd, some animation libs)
2. Library uses interior mutability the compiler can't see through (e.g., react-hook-form `watch`)
3. Profiler-measured hot path the compiler's heuristics miss (SSE-driven, ≥1 update/sec)

Use `'use no memo'` directive to exclude a single component from compilation when debugging.

> See: `docs/llm/research/frontend/react.md § React Compiler`

## useMemo for Derived Data

> **2026:** React Compiler handles most derived state automatically. Write `useMemo` only for the exceptions listed in the React Compiler section above.

Wrap filter/sort/slice/map chains with `useMemo` when the input comes from query data and the compiler exception conditions apply:

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

Token flow: `@property` → `tokens.css palette` → `tokens.css semantic (--theme-*)` → `@theme inline (Tailwind utilities)` → components

### Single Source of Truth

**All colors live in `web/app/tokens.css`. Zero exceptions.** Color changes touch exactly two layers:

| To change | Edit |
|-----------|------|
| A brand/raw color value | Layer 1 (`--palette-*`) only |
| What a semantic role maps to | Layer 2 (`--theme-*`) only |
| Add a new theme (e.g. brand X) | Layer 1 + Layer 2 only — **zero component code changes** |

If a color change requires touching `.tsx`, the policy has been violated somewhere — fix the violation, not the symptom.

### 4 Layers

| Layer | Purpose | Rule |
|-------|---------|------|
| 0. `@property --theme-*` | Type-safe `<color>` syntax + enables CSS color transitions | Register every `--theme-*` token here first |
| 1. `--palette-*` | Raw hex / OKLCH values | **Never reference from TSX — private to tokens.css** |
| 2. `--theme-*` | Semantic tokens switched via `[data-theme='dark'], .dark` | Theme switching happens here and nowhere else |
| 3. `@theme inline { --color-* }` | Tailwind v4 utility generation | Produces `bg-status-success`, `text-status-warning-fg` |

**Dark-mode selector**: use `[data-theme='dark'], .dark` dual selector for shadcn/third-party compatibility. Never rely on only one.

### Layer usage by context

| Context | Correct pattern | Wrong |
|---------|----------------|-------|
| Tailwind className | `bg-status-success`, `text-status-warning-fg` | `text-emerald-400`, `bg-green-600` |
| Inline `style={{}}` | `import { tokens } from '@/lib/design-tokens'` → `tokens.status.success` | `'var(--theme-status-success)'` raw string |
| SVG fill/stroke | `fill={tokens.status.info}` (JSX expression) | `fill="var(--theme-status-info)"` string |
| Chart gradient stopColor | `stopColor={tokens.brand.primary}` | `stopColor="var(--theme-primary)"` |
| Recharts fill/stroke | `fill={tokens.status.success}` | `fill="var(--theme-status-success)"` |
| 3rd-party CSS overrides | Dedicated `.css` file (e.g. `swagger-overrides.css`) + `import './swagger-overrides.css'` | Inline `<style>{`...`}</style>` blocks in `.tsx` |

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

### Adding a New Theme (procedure)

```
1. Layer 1 → add `--palette-{brand}-*` raw values (hex or OKLCH)
2. Layer 2 → add `[data-theme='{brand}'], .{brand} { --theme-*: ... }` block
3. Layer 0 / Layer 3 → no changes (automatic propagation)
4. web/components/theme-provider.tsx → add '{brand}' to theme option union
```

Component code is untouched. If any `.tsx` needs a change to support the new theme, the offending component is violating the single-source rule — fix the component, not the theme.

### OKLCH migration (Tailwind v4 default)

Tailwind v4 ships OKLCH palettes by default. Layer 1 values may be written in hex or OKLCH interchangeably; prefer OKLCH for wide-gamut (P3) displays. The semantic layer (Layer 2) and all component code remain color-space-agnostic.

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

All tests that create or modify resources MUST use `try/finally` to guarantee cleanup even on assertion failure:

```typescript
// UI create + API fallback cleanup (preferred for UI tests)
test('create and delete server', async ({ page, request }) => {
  const name = `e2e-${testId()}`
  const tokens = await apiLogin(request)
  const api = authedRequest(request, tokens.accessToken)

  // Create via UI
  await page.getByRole('button', { name: /register/i }).click()
  await page.getByLabel(/name/i).fill(name)
  await page.getByRole('button', { name: /register/i }).last().click()

  try {
    // Verify + interact
    await expect(page.getByText(name)).toBeVisible({ timeout: T_DEFAULT })
    // ... more assertions ...
  } finally {
    // Always cleanup via API regardless of UI assertion outcome
    const list = await api.get('/v1/resource').then(r => r.json())
    const created = list.find((s: { name: string }) => s.name === name)
    if (created) await api.delete(`/v1/resource/${created.id}`)
  }
})

// Pure API test (simpler pattern)
let createdId: string | undefined
try {
  const res = await api.post('/v1/keys', { name: `e2e-${testId()}` })
  createdId = (await res.json()).id
  // ... assertions ...
} finally {
  if (createdId) await api.delete(`/v1/keys/${createdId}`)
}
```

### API Auth Helper Pattern

Tests that need direct API calls use `apiLogin` + `authedRequest` from `web/e2e/helpers/api.ts`:

```typescript
import { apiLogin, authedRequest } from './helpers/api'

test('...', async ({ page, request }) => {
  const tokens = await apiLogin(request)      // POST /v1/auth/login
  const api = authedRequest(request, tokens.accessToken)  // adds Authorization header

  const res = await api.post('/v1/mcp/servers', { data: { name, url } })
  // use api.get / api.post / api.patch / api.delete
})
```

Fixture: always destructure `{ page, request }` — `request` is the Playwright `APIRequestContext`.

### Dialog Interaction (ConfirmDialog)

NEVER use `page.once('dialog', ...)` — this intercepts native browser dialogs, not app-rendered React modals:

```typescript
// CORRECT — query the React-rendered ConfirmDialog
const confirmDialog = page.getByRole('dialog')
await expect(confirmDialog).toBeVisible({ timeout: T_SHORT })
await confirmDialog.getByRole('button', { name: /delete|confirm/i }).last().click()
await expect(confirmDialog).not.toBeVisible({ timeout: T_DEFAULT })

// WRONG — dead code for React-managed modals
page.once('dialog', (dialog) => dialog.accept()) // ✗ only fires for window.confirm()
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

### Violations (all are P1)

| Violation | Fix |
|-----------|-----|
| Shared logic in feature dirs | Extract to `components/` or `lib/` |
| Page-specific logic in `components/` | Move to `app/{route}/components/` |
| Cross-route import (`app/A/` imports `app/B/components/`) | Lift shared dep to `components/` or duplicate per route |
| **Single-importer shared component** (file in `components/` imported by exactly one route) | Move down to `app/{route}/components/` |

### Non-Goals (do not propose these)

Atomic Design is **explicitly rejected** for this codebase:

| Rejected | Reason |
|----------|--------|
| `components/atoms/`, `components/molecules/`, `components/organisms/`, `components/templates/` | Conflicts with App Router colocation; Vercel + shadcn/ui 2026 standard is `components/ui/` primitives + `app/{route}/components/` feature folders |
| Using terms "atom/molecule/organism" in PR reviews, commits, or docs | Classification is ambiguous (is a `Button` atom or molecule?) — produces unproductive boundary debates |
| Global `organisms/` folder | Organisms are typically route-specific → global location creates orphan files on route deletion and blurs ownership |
| Renaming 4-Layer terminology to Atomic terms | Training data overwhelmingly uses `app/route/components/` pattern → keeps LLM generation accuracy high |

If new structure is needed, extend 4-Layer (add sub-folders like `app/{route}/components/modals/`), do not introduce a parallel taxonomy.

---

## i18n Compliance

| Rule | Detail |
|------|--------|
| All UI strings | Must use `t('namespace.key')` — no hardcoded English/Korean/Japanese in JSX content |
| Props included | `placeholder=`, `title=`, `aria-label=`, `label=` values that are user-visible must also use `t()` |
| Key parity | All keys in `en.json` must exist in `ko.json` and `ja.json` |
| Formatter usage | Use `fmtMs`, `fmtCompact`, `fmtPct` etc from `chart-theme.ts` — never local `toFixed`/`toLocaleString` for display |
| Missing keys | Add to all three locale files simultaneously |
| Namespace | Always `t('namespace.key')` — never top-level single-word keys |

---

## Performance Rules

| Rule | Detail |
|------|--------|
| Derived state | React Compiler handles automatically; manual `useMemo` only for compiler exceptions (see React Compiler section) |
| Handler refs | React Compiler handles automatically; manual `useCallback` only for compiler exceptions |
| Heavy panels | Modals/charts with conditional render → `dynamic(() => import(...), { ssr: false })` |
| Query dedup | Same `queryKey` in sibling components → lift to parent or share `queryOptions` factory |
| SSE-driven components | Props updated ≥1/s from SSE or `setInterval` ≤100ms → `React.memo` required (compiler exception #3) |
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

## lucide-react v1 Patterns

### `LucideProvider` — Global Icon Defaults

Set default `size`, `strokeWidth`, and `color` for all icons in a subtree without prop drilling:

```tsx
import { LucideProvider } from 'lucide-react'

// In a layout or section that uses many icons
<LucideProvider size={16} strokeWidth={1.5}>
  <Nav />
  <Sidebar />
</LucideProvider>
```

Individual icon props override the provider. Use instead of repeating `size={16}` on every icon.

### `aria-hidden` Default (v1 behavior change)

All icons now render with `aria-hidden="true"` by default. This is correct for decorative icons. For icon-only buttons or standalone status icons that convey meaning, explicitly override:

```tsx
// Decorative — no change needed (aria-hidden="true" is the default)
<Trash2 className="h-4 w-4" />

// Semantic — must override (icon conveys meaning without visible text)
<AlertCircle aria-hidden={false} aria-label={t('status.error')} className="h-4 w-4" />
```

Rule: any icon used as the *only* indicator of meaning (no adjacent text) needs `aria-hidden={false}` + `aria-label`.

### CSS Class Name Drift

When icons are renamed, lucide keeps the old import name as an alias (TypeScript keeps compiling) but the rendered SVG emits the **canonical new class name**. Example:

```tsx
import { Home } from 'lucide-react'
// Import compiles fine, but SVG emits class="lucide lucide-house" (not lucide-home)
```

**Rule**: do not use `lucide-*` CSS selectors — they are fragile across renames. Style icons via `className` prop only.

### `createLucideIcon` — Custom Icons

For custom SVG icons that should match Lucide's conventions (size, strokeWidth, color props):

```tsx
import { createLucideIcon } from 'lucide-react'

const HexIcon = createLucideIcon('hex-icon', [
  ['polygon', { points: '12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5' }],
])

// Usage — identical to any Lucide icon
<HexIcon size={20} strokeWidth={1.5} />
```

Prefer this over hand-rolling SVG components when you need size/color/strokeWidth props to work correctly.

### `DynamicIcon` — CMS-Driven Icons (bundle warning)

```tsx
import { DynamicIcon } from 'lucide-react/dynamic'
<DynamicIcon name="camera" size={24} />
```

**Warning**: this separate entry point imports all icons into the bundle and bypasses tree-shaking. Only use when icon names are genuinely unknown at build time (CMS content, user-configurable). Never use for static UI.

---

## React 19.2 Patterns

### `<Activity>` — State-Preserving Conditional Render

Replaces conditional rendering patterns that lose component state on hide/show (e.g. tab panels, back-navigation preservation):

```tsx
import { Activity } from 'react'

// Replaces: {isVisible && <ExpensivePanel />}
<Activity mode={isVisible ? 'visible' : 'hidden'}>
  <ExpensivePanel />
</Activity>
```

When `mode="hidden"`: effects unmount, updates defer (off-screen priority). When `mode="visible"`: effects mount, updates process normally. State is preserved across both modes.

Use for: tab panels that should not remount, routes that need to preserve scroll position, any panel where remounting is expensive.

### `useEffectEvent` — Stable Event Callbacks in Effects

For callbacks inside `useEffect` that need to read latest props/state without being re-listed as dependencies:

```tsx
import { useEffectEvent } from 'react'

function ChatRoom({ roomId, theme }) {
  // onConnected always reads latest `theme`, but is not a dep of the effect
  const onConnected = useEffectEvent(() => {
    showNotification('Connected!', theme)
  })

  useEffect(() => {
    const conn = createConnection(roomId)
    conn.on('connected', onConnected)
    return () => conn.disconnect()
  }, [roomId]) // no `theme` needed here — useEffectEvent handles it
}
```

**Rule**: use `useEffectEvent` instead of suppressing `react-hooks/exhaustive-deps` for event-like callbacks. Requires `eslint-plugin-react-hooks` v6 (flat config).

---

## Review Fix Priority

| Priority | Category |
|----------|----------|
| P0 (fix immediately) | Hardcoded hex, wrong token names, broken i18n keys, missing i18n parity |
| P1 (fix in same pass) | Raw `var(--theme-*)` strings, missing `useMemo`, missing `aria-label`, SSE components without `React.memo`, time-display without interval tick, `onSuccess` for invalidation (→ `onSettled`), icon-only semantic icons missing `aria-hidden={false}` |
| P2 (fix if touching file) | Component extraction for 3+ duplicates, prop count reduction, zero-value stat containers |
