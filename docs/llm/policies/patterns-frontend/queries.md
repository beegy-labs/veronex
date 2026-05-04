# Frontend Patterns — TanStack Query v5 Patterns

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns-frontend.md`](../patterns-frontend.md)

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

