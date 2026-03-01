# Data Fetching — 2026 Research

> **Last Researched**: 2026-03-01 | **Source**: TanStack Query v5 docs + verified in production
> **Status**: ✅ Verified — used in `web/hooks/use-inference-stream.ts` + all page queries

---

## TanStack Query v5 — Key Patterns

### Polling with Background Tab Optimization

```tsx
useQuery({
  queryKey: ['flow-jobs'],
  queryFn:  () => api.jobs('limit=50'),
  refetchInterval: 5_000,
  refetchIntervalInBackground: false,   // ✅ pause when tab hidden — saves CPU/network
})
```

**`refetchIntervalInBackground: false`** is the 2026 default recommendation.
Polling tabs that are backgrounded wastes resources. Use `false` unless the data must stay
fresh in the background (e.g., a notification badge in a persistent layout).

---

### staleTime vs refetchInterval

| Setting | Purpose | Use when |
|---------|---------|---------|
| `refetchInterval` | Re-fetch every N ms regardless | Real-time dashboards, live data |
| `staleTime` | Don't re-fetch if data is fresh | Slow-changing data (models list, server list) |
| Both combined | Fresh window + periodic update | History/chart data that changes slowly |

```tsx
// ✅ History data: stale for 5 min (chart won't flicker), refetched on mount
useQuery({
  queryKey: ['server-history', id],
  queryFn:  () => api.serverMetricsHistory(id, 1440),
  staleTime: 5 * 60_000,
  retry: false,
})

// ✅ Live data: always re-fetch on interval
useQuery({
  queryKey: ['live-metrics', id],
  queryFn:  () => api.serverMetrics(id),
  refetchInterval: 30_000,
  retry: false,
})
```

---

### retry: false for Non-Critical Queries

```tsx
// ✅ For non-blocking data (metrics, history) — fail fast, don't retry
useQuery({
  queryKey: ['server-metrics', id],
  queryFn:  () => api.serverMetrics(id),
  retry: false,
})
```

If the node-exporter is down, retrying 3× (default) just delays the fallback UI.
Use `retry: false` for peripheral data. Keep default retry for critical data.

---

### useQueries — Parallel Dynamic Queries

```tsx
const serverMetricQueries = useQueries({
  queries: servers.map(s => ({
    queryKey: ['server-metrics', s.id],
    queryFn:  () => api.serverMetrics(s.id),
    refetchInterval: 30_000,
    retry: false,
  })),
})
// Result: serverMetricQueries[i].data, .isLoading, .error
```

Use `useQueries` (not looping `useQuery`) for a variable-length list of parallel queries.

---

### Query Key Conventions

```tsx
// ✅ Array keys: stable, type-safe, cacheable
queryKey: ['dashboard-stats']
queryKey: ['server-metrics', serverId]
queryKey: ['usage-aggregate', hours]
queryKey: ['flow-jobs']

// ❌ String keys — harder to invalidate selectively
queryKey: `server-metrics-${serverId}`
```

---

## Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| `refetchIntervalInBackground: true` (default) | Burns CPU/network in background tabs | Set `false` for all polling queries |
| Polling in a `useEffect` with `setInterval` | No deduplication, no caching, no devtools | Use TanStack Query |
| `retry: 3` on peripheral data | Delays fallback UI by 3× | `retry: false` for non-critical queries |
| Putting query data in `useState` | Duplicates state, sync issues | Use `data` directly from `useQuery` |

---

## Sources

- [TanStack Query v5 docs](https://tanstack.com/query/latest)
- Verified: `web/hooks/use-inference-stream.ts`, `web/app/overview/page.tsx`
