import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_SLOW } from '@/lib/constants'

// ── Usage aggregate (total requests, tokens, etc.) ────────────────────────────

export const usageAggregateQuery = (hours: number) => queryOptions({
  queryKey: ['usage-aggregate', hours] as const,
  queryFn: () => api.usageAggregate(hours),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── Usage breakdown (by provider / key / model) ────────────────────────────────

export const usageBreakdownQuery = (hours: number) => queryOptions({
  queryKey: ['usage-breakdown', hours] as const,
  queryFn: () => api.usageBreakdown(hours),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── Analytics stats (TPS, models, finish reasons) ─────────────────────────────

export const analyticsQuery = (hours: number) => queryOptions({
  queryKey: ['analytics', hours] as const,
  queryFn: () => api.analytics(hours),
  staleTime: STALE_TIME_SLOW,
  refetchInterval: 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})

// ── Per-key usage ─────────────────────────────────────────────────────────────

export const keyUsageQuery = (keyId: string | null, hours: number) => queryOptions({
  queryKey: ['key-usage', keyId, hours] as const,
  queryFn: () => api.keyUsage(keyId!, hours),
  enabled: !!keyId,
  staleTime: STALE_TIME_SLOW,
  refetchInterval: 60_000,
  refetchIntervalInBackground: false,
  retry: false,
})

export const keyModelBreakdownQuery = (keyId: string | null, hours: number) => queryOptions({
  queryKey: ['key-model-breakdown', keyId, hours] as const,
  queryFn: () => api.keyModelBreakdown(keyId!, hours),
  enabled: !!keyId,
  staleTime: STALE_TIME_SLOW,
  retry: false,
})
