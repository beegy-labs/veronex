import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW, REFETCH_INTERVAL_SLOW } from '@/lib/constants'

// ── API keys list ─────────────────────────────────────────────────────────────
// staleTime slightly below refetchInterval prevents stale flash on each poll tick.

export const keysQuery = queryOptions({
  queryKey: ['keys'] as const,
  queryFn: () => api.keys(),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: REFETCH_INTERVAL_SLOW,
  refetchIntervalInBackground: false,
})
