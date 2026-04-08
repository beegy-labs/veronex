# TanStack Query v5 — Research

> **Last Researched**: 2026-04-07 | **Source**: Official docs + web search + implementation
> **Status**: verified — patterns used across all 13 pages
> **Companion**: `research/frontend/tanstack-query-advanced.md` — suspense, prefetch, parallel queries

---

## `queryOptions()` — The Central Pattern (2026)

Define query configuration **once** in a central location and share it across `useQuery`, `prefetchQuery`, and `ensureQueryData`:

```ts
// web/lib/queries/dashboard.ts
import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

export const dashboardStatsQuery = queryOptions({
  queryKey: ['dashboard', 'stats'],
  queryFn: () => api.stats(),
  staleTime: 30_000,
  retry: false,
})

export const jobsQuery = (params?: string) => queryOptions({
  queryKey: ['dashboard', 'jobs', params],
  queryFn: () => api.jobs(params),
  staleTime: 5_000,
})
```

```ts
// In a component
const { data } = useQuery(dashboardStatsQuery)
const { data: jobs } = useQuery(jobsQuery('status=completed'))

// In a router loader (prefetch before navigation)
await queryClient.prefetchQuery(dashboardStatsQuery)
```

Benefits: single config SSOT, type-safe key sharing, reusable in `prefetchQuery`/`useSuspenseQuery`.

---

## Directory Convention

```
web/lib/queries/
├── index.ts            # re-exports all queryOptions
├── dashboard.ts        # stats, jobs, performance, analytics
├── keys.ts             # API keys
├── usage.ts            # usage aggregate, breakdown, key usage
├── servers.ts          # GPU servers, metrics
├── providers.ts        # Ollama models, Gemini policies
├── accounts.ts         # accounts, sessions
└── capacity.ts         # capacity, capacity settings
```

**Status:** Not yet created. All queries currently inline in page components.

---

## staleTime + refetchInterval Co-location

Always pair `staleTime` with `refetchInterval` to avoid stale flash between polls:

```ts
// CORRECT — staleTime slightly less than refetchInterval prevents stale UI between polls
queryOptions({
  queryKey: ['dashboard', 'jobs'],
  queryFn: () => api.jobs(),
  staleTime: 4_900,          // 4.9s — treat data as fresh for 4.9s
  refetchInterval: 5_000,    // poll every 5s
  refetchIntervalInBackground: false,  // pause when tab hidden
})

```

---

## Staleness Strategy by Data Type

| Data Type | `staleTime` | `refetchInterval` | Notes |
|---|---|---|---|
| Dashboard stats (KPI) | `30_000` | `30_000` | KPIs don't need sub-second updates |
| Live jobs list | `4_900` | `5_000` | Active polling for running jobs |
| API keys | `60_000` | — | Mostly static, refetch on mutation |
| Server metrics | `5_000` | `10_000` | GPU metrics every 10s |
| Capacity state | `10_000` | `30_000` | Capacity analyzer runs every 30s |
| Gemini policies | `Infinity` | — | Config data, only refetch on edit |

---

## Invalidation Strategy

```ts
// After mutation, invalidate by prefix (all queries starting with ['dashboard'])
onSettled: () => queryClient.invalidateQueries({ queryKey: ['dashboard'] })

// Or by exact key
onSettled: () => queryClient.invalidateQueries({ queryKey: ['keys'], exact: true })
```

**Rule:** Always use `onSettled` (not `onSuccess`) — runs on both success and failure, ensuring reconciliation.

---

## Optimistic Updates Pattern

For toggle mutations (key active/inactive, provider enable/disable):

```ts
useMutation({
  mutationFn: (id: string) => api.toggleKeyActive(id),

  onMutate: async (id) => {
    // 1. Cancel any outgoing refetches to avoid overwriting optimistic update
    await queryClient.cancelQueries({ queryKey: ['keys'] })

    // 2. Snapshot current value for rollback
    const snapshot = queryClient.getQueryData<ApiKey[]>(['keys'])

    // 3. Optimistically update the cache
    queryClient.setQueryData<ApiKey[]>(['keys'], old =>
      old?.map(k => k.id === id ? { ...k, is_active: !k.is_active } : k) ?? []
    )

    return { snapshot }  // returned as context
  },

  onError: (_err, _id, ctx) => {
    // Rollback on error
    if (ctx?.snapshot) {
      queryClient.setQueryData(['keys'], ctx.snapshot)
    }
  },

  onSettled: () => {
    // Always reconcile with server (catches partial failures)
    queryClient.invalidateQueries({ queryKey: ['keys'] })
  },
})
```

---

## Sources

- TanStack Query v5 docs, `queryOptions` API reference
- Verified: `web/hooks/use-inference-stream.ts`, all pages in `web/app/`
