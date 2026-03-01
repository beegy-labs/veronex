import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

// ── GPU servers list ──────────────────────────────────────────────────────────

export const serversQuery = queryOptions({
  queryKey: ['servers'] as const,
  queryFn: () => api.servers(),
  staleTime: 59_000,
  refetchInterval: 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── Live node-exporter metrics for a single server ────────────────────────────

export const serverMetricsQuery = (serverId: string) => queryOptions({
  queryKey: ['server-metrics', serverId] as const,
  queryFn: () => api.serverMetrics(serverId),
  staleTime: 29_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── ClickHouse power history for a single server ──────────────────────────────
// Long history windows change slowly — 5 min refetch is sufficient.

export const serverMetricsHistoryQuery = (serverId: string, minutes = 1440) => queryOptions({
  queryKey: ['server-metrics-history', serverId, minutes] as const,
  queryFn: () => api.serverMetricsHistory(serverId, minutes),
  staleTime: 5 * 60_000 - 1_000,
  refetchInterval: 5 * 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})
