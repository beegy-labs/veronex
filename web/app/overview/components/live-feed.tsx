'use client'

import { memo, useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from '@/i18n'
import { Loader2, Clock } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { api } from '@/lib/api'
import { tokens } from '@/lib/design-tokens'

function statusDotColor(status: string): string {
  switch (status) {
    case 'running':   return tokens.status.info
    case 'pending':   return tokens.status.warning
    case 'completed': return tokens.status.success
    case 'failed':    return tokens.status.error
    default:          return tokens.status.warning
  }
}

function StatusIcon({ status }: { status: string }) {
  const cls = 'h-3 w-3 shrink-0'
  if (status === 'running') return <Loader2 className={`${cls} animate-spin`} />
  return <Clock className={cls} />
}

function ElapsedTime({ since }: { since: string }) {
  const [, tick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => tick(n => n + 1), 1_000)
    return () => clearInterval(id)
  }, [])
  const sec = Math.max(0, Math.round((Date.now() - new Date(since).getTime()) / 1000))
  if (sec < 60) return <span>{sec}s</span>
  return <span>{Math.floor(sec / 60)}m {sec % 60}s</span>
}

/* ─── component ───────────────────────────────────────────── */
export const LiveFeed = memo(function LiveFeed() {
  const { t } = useTranslation()

  // Fetch active jobs from DB — source of truth.
  // Refetch every 2s for near-real-time, and also on new SSE events.
  const { data } = useQuery({
    queryKey: ['active-jobs'] as const,
    queryFn: () => api.jobs('status=pending,running&limit=50'),
    staleTime: 1_000,
    refetchInterval: 2_000,
    refetchIntervalInBackground: false,
  })

  const activeJobs = data?.jobs ?? []

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold">{t('overview.liveFeed')}</CardTitle>
          {activeJobs.length > 0 && (
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span
                className="h-1.5 w-1.5 rounded-full animate-pulse"
                style={{ background: tokens.status.success }}
              />
              {t('overview.liveIndicator')}
            </span>
          )}
        </div>
      </CardHeader>
      <CardContent className="p-0">
        {activeJobs.length === 0 ? (
          <div className="flex items-center justify-center h-20 text-sm text-muted-foreground">
            {t('overview.waitingRequests')}
          </div>
        ) : (
          <div className="overflow-y-auto max-h-64">
            <table className="w-full text-xs" style={{ minWidth: 480 }}>
              <tbody>
                {activeJobs.map(job => (
                  <tr
                    key={job.id}
                    className="border-b border-border last:border-0 hover:bg-muted/30 transition-colors"
                  >
                    <td className="py-2 pl-4 w-5">
                      <span
                        className="h-2 w-2 rounded-full inline-block"
                        style={{ background: statusDotColor(job.status) }}
                      />
                    </td>
                    <td className="py-2 px-2 font-mono max-w-[160px] truncate text-foreground">
                      {job.model_name}
                    </td>
                    <td className="py-2 px-2 text-muted-foreground">
                      {job.provider_name ?? job.provider_type}
                    </td>
                    <td className="py-2 px-2" style={{ color: statusDotColor(job.status) }}>
                      <span className="flex items-center gap-1">
                        <StatusIcon status={job.status} />
                        {t(`jobs.statuses.${job.status}` as Parameters<typeof t>[0])}
                      </span>
                    </td>
                    <td className="py-2 pl-2 pr-4 tabular-nums text-muted-foreground text-right whitespace-nowrap">
                      <ElapsedTime since={job.created_at} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  )
})
