# TanStack Query v5 — Research

> **Last Researched**: 2026-03-02 | **Source**: Official docs + web search + implementation
> **Status**: ✅ Verified — patterns used across all 13 pages

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

**Benefits over inline `useQuery` calls:**
- Single place to change `staleTime`, `retry`, `gcTime` for an endpoint
- Type-safe query key sharing (no string duplication)
- Reuse in `prefetchQuery` without duplicating config
- Works with `useSuspenseQuery` without extra setup

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

**Current state:** This directory does not yet exist. All queries are inline in page components. **Phase 3** of the optimization plan will create this structure.

---

## staleTime + refetchInterval Co-location

Always define `staleTime` and `refetchInterval` together to avoid "stale flash" on each poll tick:

```ts
// CORRECT — staleTime slightly less than refetchInterval prevents stale UI between polls
queryOptions({
  queryKey: ['dashboard', 'jobs'],
  queryFn: () => api.jobs(),
  staleTime: 4_900,          // 4.9s — treat data as fresh for 4.9s
  refetchInterval: 5_000,    // poll every 5s
  refetchIntervalInBackground: false,  // pause when tab hidden
})

// WRONG — staleTime 0 causes "stale" indicator to flash between polls
queryOptions({
  staleTime: 0,
  refetchInterval: 5_000,
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

**Always use `onSettled`** (not `onSuccess`) for cache invalidation — it runs whether the mutation succeeded or failed, ensuring the UI always reconciles.

```ts
useMutation({
  mutationFn: api.deleteKey,
  onSettled: () => queryClient.invalidateQueries({ queryKey: ['keys'] }),
})
```

---

## Optimistic Updates Pattern

For toggle mutations (key active/inactive, backend enable/disable):

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

## `useQueries` for Parallel Queries

Avoid looping `useQuery` — use `useQueries` for dynamic parallel fetches:

```ts
// WRONG — hooks in loops are forbidden in React
backends.map(b => useQuery({ queryKey: ['metrics', b.id], ... }))

// CORRECT — useQueries for dynamic parallel fetches
const results = useQueries({
  queries: backends.map(b => ({
    queryKey: ['server-metrics', b.id],
    queryFn: () => api.serverMetrics(b.id),
    staleTime: 5_000,
  }))
})
```

---

## Query Key Conventions

```ts
// Format: [domain, ...specifics]
['dashboard', 'stats']              // GET /v1/dashboard/stats
['dashboard', 'jobs', params]       // GET /v1/dashboard/jobs?{params}
['dashboard', 'jobs', id]           // GET /v1/dashboard/jobs/{id}
['keys']                            // GET /v1/keys
['servers']                         // GET /v1/servers
['servers', id, 'metrics']          // GET /v1/servers/{id}/metrics
['servers', id, 'metrics', 'history'] // GET /v1/servers/{id}/metrics/history
['accounts']                        // GET /v1/accounts
['capacity']                        // GET /v1/dashboard/capacity
['capacity', 'settings']            // GET /v1/dashboard/capacity/settings
```

**Rule:** Invalidating `['servers']` also invalidates `['servers', id, 'metrics']` (prefix matching). Use exact keys when you want surgical invalidation.

---

## Anti-Patterns

| Anti-pattern | Correct approach |
|---|---|
| Duplicated `queryKey` in multiple components | `queryOptions()` factory, shared from `lib/queries/` |
| `refetchInterval` without `staleTime` | Always pair them |
| `onSuccess` for cache invalidation | Use `onSettled` (runs on error too) |
| `useQuery` inside a loop | Use `useQueries` |
| Storing `data` from `useQuery` in `useState` | Read from cache directly via `queryClient.getQueryData` |
| Background polling when tab hidden | Set `refetchIntervalInBackground: false` |
| `retry: 3` (default) for non-critical data | Set `retry: false` for peripheral data (metrics, charts) |

---

## Sources

- TanStack Query v5 docs: https://tanstack.com/query/v5/docs
- `queryOptions` API: https://tanstack.com/query/v5/docs/react/reference/queryOptions
- Verified: `web/hooks/use-inference-stream.ts`, all pages in `web/app/`
