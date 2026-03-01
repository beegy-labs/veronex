import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

// ── Dashboard stats (KPI cards) ───────────────────────────────────────────────

export const dashboardStatsQuery = queryOptions({
  queryKey: ['dashboard-stats'] as const,
  queryFn: () => api.stats(),
  staleTime: 29_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
})

// ── Recent jobs (overview sidebar) ────────────────────────────────────────────

export const recentJobsQuery = queryOptions({
  queryKey: ['recent-jobs'] as const,
  queryFn: () => api.jobs('limit=10'),
  staleTime: 29_000,
  refetchInterval: 30_000,
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
}

export const dashboardJobsQuery = (p: JobsQueryParams) => queryOptions({
  queryKey: ['dashboard-jobs', p.source, p.page, p.status, p.query] as const,
  queryFn: () => {
    const qs = new URLSearchParams({
      limit: String(p.pageSize),
      offset: String(p.page * p.pageSize),
      source: p.source,
    })
    if (p.status !== 'all') qs.set('status', p.status)
    if (p.query.trim()) qs.set('q', p.query.trim())
    return api.jobs(qs.toString())
  },
  staleTime: 29_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
})

// ── Performance metrics ───────────────────────────────────────────────────────
// refetchInterval scales with the window: longer windows change less frequently.

const PERF_REFETCH: Record<number, number> = {
  24:  60_000,       // 1 min  — daily view changes frequently
  168: 5 * 60_000,  // 5 min  — weekly view
  720: 10 * 60_000, // 10 min — monthly view
}

export const performanceQuery = (hours: number) => queryOptions({
  queryKey: ['performance', hours] as const,
  queryFn: () => api.performance(hours),
  staleTime: (PERF_REFETCH[hours] ?? 60_000) - 1_000,
  refetchInterval: PERF_REFETCH[hours] ?? 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})
