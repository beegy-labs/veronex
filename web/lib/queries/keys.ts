import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

// ── API keys list ─────────────────────────────────────────────────────────────
// staleTime slightly below refetchInterval prevents stale flash on each poll tick.

export const keysQuery = queryOptions({
  queryKey: ['keys'] as const,
  queryFn: () => api.keys(),
  staleTime: 59_000,
  refetchInterval: 60_000,
  refetchIntervalInBackground: false,
})
