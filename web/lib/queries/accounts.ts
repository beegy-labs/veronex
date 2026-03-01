import { queryOptions } from '@tanstack/react-query'
import { api } from '@/lib/api'

// ── Accounts list ─────────────────────────────────────────────────────────────

export const accountsQuery = queryOptions({
  queryKey: ['accounts'] as const,
  queryFn: () => api.accounts(),
  staleTime: Infinity,
})

// ── Sessions for a specific account ──────────────────────────────────────────

export const accountSessionsQuery = (accountId: string | null, open: boolean) => queryOptions({
  queryKey: ['sessions', accountId] as const,
  queryFn: () => api.accountSessions(accountId!),
  enabled: open && !!accountId,
  staleTime: 30_000,
  retry: false,
})
