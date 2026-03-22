import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW, STALE_TIME_FAST, STALE_TIME_HISTORY, REFETCH_INTERVAL_FAST, REFETCH_INTERVAL_SLOW, REFETCH_INTERVAL_HISTORY, withJitter } from '@/lib/constants'

// ── GPU servers list ──────────────────────────────────────────────────────────

export const serversQuery = (params?: { search?: string; page?: number; limit?: number }) => queryOptions({
  queryKey: ['servers', params] as const,
  queryFn: () => api.servers(params),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_SLOW, 10_000),
  refetchIntervalInBackground: false,
  retry: false,
})

// ── Batch node-exporter metrics for the overview dashboard ────────────────────
// Single request replaces N individual /metrics calls when servers.length > 1.

export const serverMetricsBatchQuery = (serverIds: string[]) => queryOptions({
  queryKey: ['server-metrics-batch', serverIds] as const,
  queryFn: () => serverIds.length > 0 ? api.serverMetricsBatch(serverIds) : Promise.resolve({} as Record<string, import('@/lib/types').NodeMetrics>),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
  retry: false,
  enabled: serverIds.length > 0,
})

// ── Live node-exporter metrics for a single server ────────────────────────────

export const serverMetricsQuery = (serverId: string) => queryOptions({
  queryKey: ['server-metrics', serverId] as const,
  queryFn: () => api.serverMetrics(serverId),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
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
