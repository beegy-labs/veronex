import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST, REFETCH_INTERVAL_FAST, withJitter } from '@/lib/constants'

export interface ConversationsQueryParams {
  page: number
  pageSize: number
  source?: string
  search?: string
}

export const conversationsQuery = (p: ConversationsQueryParams) => queryOptions({
  queryKey: ['conversations', p.page, p.source, p.search] as const,
  queryFn: () => {
    const qs = new URLSearchParams({
      limit: String(p.pageSize),
      offset: String(p.page * p.pageSize),
    })
    if (p.source) qs.set('source', p.source)
    if (p.search) qs.set('search', p.search)
    return api.conversations(qs.toString())
  },
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
})

export const conversationDetailQuery = (id: string) => queryOptions({
  queryKey: ['conversation-detail', id] as const,
  queryFn: () => api.conversation(id),
  staleTime: STALE_TIME_FAST,
  enabled: !!id,
})

export const turnInternalsQuery = (convId: string, jobId: string, enabled: boolean) => queryOptions({
  queryKey: ['turn-internals', convId, jobId] as const,
  queryFn: () => api.turnInternals(convId, jobId),
  staleTime: Infinity,
  enabled,
})
