# Frontend Patterns — TanStack Query v5 (Advanced)

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)
> Core factory patterns: [`queries.md`](queries.md)

## TanStack Query v5 — Advanced Hooks & Mutation

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

