import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST } from '@/lib/constants'

// ── Audit events ──────────────────────────────────────────────────────────────

// action / resourceType: pass the raw UI filter value ('all' is mapped → undefined here).
export const auditQuery = (action: string, resourceType: string) => queryOptions({
  queryKey: ['audit', action, resourceType] as const,
  queryFn: () => api.auditEvents({
    limit: 200,
    action: action !== 'all' ? action : undefined,
    resource_type: resourceType !== 'all' ? resourceType : undefined,
  }),
  staleTime: STALE_TIME_FAST,
  retry: false,
})
