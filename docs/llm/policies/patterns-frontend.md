# Code Patterns: Frontend — 2026 Reference

> SSOT | **Last Updated**: 2026-03-16 | Classification: Operational | Exception: >200 lines (pattern registry)
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
