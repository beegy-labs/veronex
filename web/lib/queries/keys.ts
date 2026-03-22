import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW, REFETCH_INTERVAL_SLOW, withJitter } from '@/lib/constants'

// ── API keys list ─────────────────────────────────────────────────────────────
// staleTime slightly below refetchInterval prevents stale flash on each poll tick.

export const keysQuery = (params?: { search?: string; page?: number; limit?: number }) => queryOptions({
  queryKey: ['keys', params] as const,
  queryFn: () => api.keys(params),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_SLOW, 10_000),
  refetchIntervalInBackground: false,
})
