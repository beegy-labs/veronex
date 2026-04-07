# TanStack Query v5 — Advanced Patterns

> **Last Researched**: 2026-04-07 | **Source**: TanStack Query v5 docs + web search
> **Companion**: `research/frontend/tanstack-query.md` — core patterns

---

## Suspense Hooks (v5 — dedicated hooks, not option)

`{ suspense: true }` option is **removed** in v5. Use dedicated hooks:

| Hook | Use when |
|------|----------|
| `useSuspenseQuery` | Single query — throws to nearest `<Suspense>` boundary |
| `useSuspenseQueries` | Parallel queries — all-or-nothing suspend |
| `useSuspenseInfiniteQuery` | Infinite list with Suspense |

Components only render when data is guaranteed — no `status === 'pending'` checks needed.

```tsx
// Must wrap with <Suspense> + Error Boundary
function Article({ id }: { id: string }) {
  const { data } = useSuspenseQuery(articleQuery(id)) // never undefined
  return <h1>{data.title}</h1>
}
```

> **This codebase:** All pages use `useQuery` with `isLoading` guards (correct for polling architecture). Use `useSuspenseQuery` only when adopting Server Component streaming.

---

## Prefetch Streaming (v5.40.0+)

Pending prefetches can be dehydrated and streamed — no `await` needed:

```tsx
// Server Component — fire-and-forget
const queryClient = new QueryClient()
void queryClient.prefetchQuery(articleQuery(id)) // not awaited

return (
  <HydrationBoundary state={dehydrate(queryClient)}>
    <Suspense fallback={<Spinner />}>
      <Article id={id} /> {/* useSuspenseQuery inside — streams when ready */}
    </Suspense>
  </HydrationBoundary>
)
```

---

## `usePrefetchQuery` — Prefetch During Render

Kick off a fetch in a parent before a Suspense boundary:

```tsx
function ParentPage({ id }) {
  usePrefetchQuery(articleQuery(id)) // fires on render, doesn't suspend
  return (
    <Suspense fallback={<Skeleton />}>
      <Article id={id} />
    </Suspense>
  )
}
```

---

## `useQueries` for Parallel Queries

Never use `useQuery` in a loop:

```ts
const results = useQueries({
  queries: providers.map(p => ({
    queryKey: ['server-metrics', p.id],
    queryFn: () => api.serverMetrics(p.id),
    staleTime: 5_000,
  }))
})
```

---

## Query Key Conventions

```ts
// Format: [domain, ...specifics]
['dashboard', 'stats']                  // GET /v1/dashboard/stats
['dashboard', 'jobs', params]           // GET /v1/dashboard/jobs?{params}
['keys']                                // GET /v1/keys
['servers']                             // GET /v1/servers
['servers', id, 'metrics']              // GET /v1/servers/{id}/metrics
['servers', id, 'metrics', 'history']   // GET /v1/servers/{id}/metrics/history
['capacity']                            // GET /v1/dashboard/capacity
['capacity', 'settings']                // GET /v1/dashboard/capacity/settings
```

Invalidating `['servers']` also invalidates `['servers', id, 'metrics']` (prefix matching). Use exact keys for surgical invalidation.

---

## Anti-Patterns

| Anti-pattern | Correct approach |
|---|---|
| Duplicated `queryKey` in multiple components | `queryOptions()` factory from `lib/queries/` |
| `refetchInterval` without `staleTime` | Always pair them |
| `onSuccess` for cache invalidation | Use `onSettled` (runs on error too) |
| `useQuery` inside a loop | Use `useQueries` |
| Storing `data` from `useQuery` in `useState` | Read from cache via `queryClient.getQueryData` |
| Background polling when tab hidden | Set `refetchIntervalInBackground: false` |
| `retry: 3` (default) for non-critical data | Set `retry: false` for peripheral data |

---

## Sources

- TanStack Query v5 docs, `queryOptions` API reference
- Verified: `web/hooks/use-inference-stream.ts`, all pages in `web/app/`
