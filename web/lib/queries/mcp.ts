import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST, REFETCH_INTERVAL_FAST, withJitter } from '@/lib/constants'

export const mcpServersQuery = () => queryOptions({
  queryKey: ['mcp-servers'] as const,
  queryFn: () => api.mcpServers(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
})
