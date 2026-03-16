import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST, REFETCH_INTERVAL_FAST, REFETCH_INTERVAL_SLOW } from '@/lib/constants'

// ── Dashboard overview (aggregated snapshot) ──────────────────────────────────

export const dashboardOverviewQuery = queryOptions({
  queryKey: ['dashboard-overview'] as const,
  queryFn: () => api.overview(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
})

// ── Dashboard stats (KPI cards) ───────────────────────────────────────────────

export const dashboardStatsQuery = queryOptions({
  queryKey: ['dashboard-stats'] as const,
  queryFn: () => api.stats(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
})

// ── Recent jobs (overview sidebar) ────────────────────────────────────────────

export const recentJobsQuery = queryOptions({
  queryKey: ['recent-jobs'] as const,
  queryFn: () => api.jobs('limit=10'),
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
})

// ── Paginated jobs (jobs page) ────────────────────────────────────────────────
// Accepts structured params so the queryKey is granular and cache-invalidation
// by source/status/page works correctly.

export interface JobsQueryParams {
  source: string
  page: number
  status: string
  query: string
  pageSize: number
  model?: string
  provider?: string
}

export const dashboardJobsQuery = (p: JobsQueryParams) => queryOptions({
  queryKey: ['dashboard-jobs', p.source, p.page, p.status, p.query, p.model ?? '', p.provider ?? ''] as const,
  queryFn: () => {
    const qs = new URLSearchParams({
      limit: String(p.pageSize),
      offset: String(p.page * p.pageSize),
      source: p.source,
    })
    if (p.status !== 'all') qs.set('status', p.status)
    if (p.query.trim()) qs.set('q', p.query.trim())
    if (p.model) qs.set('model', p.model)
    if (p.provider) qs.set('provider', p.provider)
    return api.jobs(qs.toString())
  },
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
})

// ── Queue depth (live — 3 s poll) ─────────────────────────────────────────────

export const queueDepthQuery = queryOptions({
  queryKey: ['queue-depth'] as const,
  queryFn: () => api.queueDepth(),
  staleTime: 2_000,
  refetchInterval: 3_000,
  refetchIntervalInBackground: false,
})

// ── Performance metrics ───────────────────────────────────────────────────────
// refetchInterval scales with the window: longer windows change less frequently.

const PERF_REFETCH: Record<number, number> = {
  24:  REFETCH_INTERVAL_SLOW,       // 1 min  — daily view changes frequently
  168: 5 * REFETCH_INTERVAL_SLOW,   // 5 min  — weekly view
  720: 10 * REFETCH_INTERVAL_SLOW,  // 10 min — monthly view
}

export const performanceQuery = (hours: number) => queryOptions({
  queryKey: ['performance', hours] as const,
  queryFn: () => api.performance(hours),
  staleTime: (PERF_REFETCH[hours] ?? REFETCH_INTERVAL_SLOW) - 1_000,
  refetchInterval: PERF_REFETCH[hours] ?? REFETCH_INTERVAL_SLOW,
  refetchIntervalInBackground: false,
  retry: false,
})
