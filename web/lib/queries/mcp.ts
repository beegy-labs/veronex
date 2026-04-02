import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST, STALE_TIME_SLOW, REFETCH_INTERVAL_FAST, withJitter } from '@/lib/constants'

export const mcpServersQuery = () => queryOptions({
  queryKey: ['mcp-servers'] as const,
  queryFn: () => api.mcpServers(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
})

// ── MCP call statistics (analytics) ──────────────────────────────────────────

export const mcpStatsQuery = (hours: number) => queryOptions({
  queryKey: ['mcp-stats', hours] as const,
  queryFn: () => api.mcpStats(hours),
  staleTime: STALE_TIME_SLOW,
})

// ── Lab settings (used by OrchestratorModelSelector) ─────────────────────────

export const labSettingsQuery = queryOptions({
  queryKey: ['lab-settings'] as const,
  queryFn: () => api.labSettings(),
  staleTime: STALE_TIME_SLOW,
})
