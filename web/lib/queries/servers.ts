import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW, STALE_TIME_FAST, STALE_TIME_HISTORY, REFETCH_INTERVAL_FAST, REFETCH_INTERVAL_SLOW, REFETCH_INTERVAL_HISTORY } from '@/lib/constants'

// ── GPU servers list ──────────────────────────────────────────────────────────

export const serversQuery = (params?: { search?: string; page?: number; limit?: number }) => queryOptions({
  queryKey: ['servers', params] as const,
  queryFn: () => api.servers(params),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: REFETCH_INTERVAL_SLOW,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── Live node-exporter metrics for a single server ────────────────────────────

export const serverMetricsQuery = (serverId: string) => queryOptions({
  queryKey: ['server-metrics', serverId] as const,
  queryFn: () => api.serverMetrics(serverId),
  staleTime: STALE_TIME_FAST,
  refetchInterval: REFETCH_INTERVAL_FAST,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── ClickHouse power history for a single server ──────────────────────────────
// Long history windows change slowly — 5 min refetch is sufficient.

export const serverMetricsHistoryQuery = (serverId: string, hours = 1440) => queryOptions({
  queryKey: ['server-metrics-history', serverId, hours] as const,
  queryFn: () => api.serverMetricsHistory(serverId, hours),
  staleTime: STALE_TIME_HISTORY,
  refetchInterval: REFETCH_INTERVAL_HISTORY,
  refetchIntervalInBackground: false,
  retry: false,
})
