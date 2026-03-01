import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

// ── Live job stream for network flow visualization ─────────────────────────────
// Polls every 5s; staleTime slightly below to prevent stale flash on each tick.

export const flowJobsQuery = queryOptions({
  queryKey: ['flow-jobs'] as const,
  queryFn: () => api.jobs('limit=50'),
  staleTime: 4_900,
  refetchInterval: 5_000,
  refetchIntervalInBackground: false,
})
