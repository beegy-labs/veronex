import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { STALE_TIME_FAST } from '@/lib/constants'

// ── Accounts list ─────────────────────────────────────────────────────────────

export const accountsQuery = queryOptions({
  queryKey: ['accounts'] as const,
  queryFn: () => api.accounts(),
  staleTime: Infinity,
})

// ── Roles ────────────────────────────────────────────────────────────────────

export const rolesQuery = queryOptions({
  queryKey: ['roles'] as const,
  queryFn: () => api.roles(),
  staleTime: Infinity,
})

// ── Sessions for a specific account ──────────────────────────────────────────

export const accountSessionsQuery = (accountId: string | null, open: boolean) => queryOptions({
  queryKey: ['sessions', accountId] as const,
  queryFn: () => api.accountSessions(accountId!),
  enabled: open && !!accountId,
  staleTime: STALE_TIME_FAST,
  retry: false,
})
