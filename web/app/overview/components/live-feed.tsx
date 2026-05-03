'use client'

import { memo, useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { activeJobsQuery } from '@/lib/queries'
import { useTranslation } from '@/i18n'
import { Loader2, Clock } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
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
  const cls = 'vds-h-3 vds-w-3 vds-flex-shrink-0'
  if (status === 'running') return <Loader2 className={`${cls} vds-animate-spin`} />
  return <Clock className={cls} />
}

const ElapsedTime = memo(function ElapsedTime({ since }: { since: string }) {
  const [, tick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => tick(n => n + 1), 1_000)
    return () => clearInterval(id)
  }, [])
  const sec = Math.max(0, Math.round((Date.now() - new Date(since).getTime()) / 1000))
  if (sec < 60) return <span>{sec}s</span>
  return <span>{Math.floor(sec / 60)}m {sec % 60}s</span>
})

/* ─── component ───────────────────────────────────────────── */
export const LiveFeed = memo(function LiveFeed() {
  const { t } = useTranslation()

  // Fetch active jobs from DB — source of truth.
  // Refetch every 2s for near-real-time, and also on new SSE events.
  const { data } = useQuery(activeJobsQuery)

  const activeJobs = data?.jobs ?? []

  return (
    <Card>
      <CardHeader className="vds-pb-2">
        <div className="vds-flex vds-items-center vds-justify-between">
          <CardTitle className="vds-text-sm vds-font-600">{t('overview.liveFeed')}</CardTitle>
          {activeJobs.length > 0 && (
            <span className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-dim">
              <span
                className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-animate-pulse"
                style={{ background: tokens.status.success }}
              />
              {t('overview.liveIndicator')}
            </span>
          )}
        </div>
      </CardHeader>
      <CardContent className="vds-p-0">
        {activeJobs.length === 0 ? (
          <div className="vds-flex vds-items-center vds-justify-center vds-h-20 vds-text-sm vds-text-dim">
            {t('overview.waitingRequests')}
          </div>
        ) : (
          <div className="vds-overflow-y-auto vds-max-h-64">
            <Table className="vds-text-xs" style={{ minWidth: 480 }}>
              <TableBody>
                {activeJobs.map(job => (
                  <TableRow
                    key={job.id}
                    className="vds-border-b-1 vds-border-subtle last:border-0 vds-hover:bg-hover/30 vds-transition-colors"
                  >
                    <TableCell className="vds-py-2 vds-pl-4 vds-w-5">
                      <span
                        className="vds-h-2 vds-w-2 vds-rounded-full vds-inline-block"
                        style={{ background: statusDotColor(job.status) }}
                      />
                    </TableCell>
                    <TableCell className="vds-py-2 vds-px-2 vds-font-mono vds-max-w-[160px] vds-truncate vds-text-primary">
                      {job.model_name}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-px-2 vds-text-dim">
                      {job.provider_name ?? job.provider_type}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-px-2" style={{ color: statusDotColor(job.status) }}>
                      <span className="vds-flex vds-items-center vds-gap-1">
                        <StatusIcon status={job.status} />
                        {t(`jobs.statuses.${job.status}` as Parameters<typeof t>[0])}
                      </span>
                    </TableCell>
                    <TableCell className="vds-py-2 vds-pl-2 vds-pr-4 vds-tabular-nums vds-text-dim vds-text-right vds-whitespace-nowrap">
                      <ElapsedTime since={job.created_at} />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  )
})
