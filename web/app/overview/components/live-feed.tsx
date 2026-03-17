'use client'

import { memo, useEffect, useMemo, useState } from 'react'
import { useTranslation } from '@/i18n'
import type { FlowEvent } from '@/hooks/use-inference-stream'
import { CheckCircle2, XCircle, Loader2, Ban, Clock } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { fmtMsNullable } from '@/lib/chart-theme'
import { fmtTimeAgo } from '@/lib/date'
import { tokens } from '@/lib/design-tokens'

function statusDotColor(status: string): string {
  switch (status) {
    case 'completed': return tokens.status.success
    case 'failed':    return tokens.status.error
    case 'running':   return tokens.status.info
    case 'cancelled': return tokens.status.cancelled
    default:          return tokens.status.warning
  }
}

function StatusIcon({ status }: { status: string }) {
  const cls = 'h-3 w-3 shrink-0'
  switch (status) {
    case 'completed': return <CheckCircle2 className={cls} />
    case 'failed':    return <XCircle className={cls} />
    case 'running':   return <Loader2 className={`${cls} animate-spin`} />
    case 'cancelled': return <Ban className={cls} />
    default:          return <Clock className={cls} />
  }
}

/* ─── component ───────────────────────────────────────────── */
interface Props {
  events: FlowEvent[]
}

export const LiveFeed = memo(function LiveFeed({ events }: Props) {
  const { t } = useTranslation()

  // Tick every 10s so displayed "X ago" labels age without waiting for new events.
  const [, tick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => tick(n => n + 1), 10_000)
    return () => clearInterval(id)
  }, [])

  // Show only enqueue-phase events (job arrivals) — but display CURRENT status.
  // events are newest-first, so the first occurrence of a jobId is its latest state.
  const feedEvents = useMemo(
    () => events.filter(e => e.phase === 'enqueue'),
    [events],
  )

  // Latest status + latency per jobId — updated by dispatch/response events.
  // events newest-first → first match = most recent state for that job.
  const latestByJob = useMemo(() => {
    const map = new Map<string, { status: string; latencyMs: number | null }>()
    for (const e of events) {
      if (!map.has(e.jobId)) {
        map.set(e.jobId, { status: e.status, latencyMs: e.latencyMs })
      }
    }
    return map
  }, [events])

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold">{t('overview.liveFeed')}</CardTitle>
          {feedEvents.length > 0 && (
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
        {feedEvents.length === 0 ? (
          <div className="flex items-center justify-center h-20 text-sm text-muted-foreground">
            {t('overview.waitingRequests')}
          </div>
        ) : (
          <div className="overflow-y-auto max-h-64">
            <table className="w-full text-xs" style={{ minWidth: 480 }}>
              <tbody>
                {feedEvents.map(ev => {
                  // Use latest known state (dispatch/response update this as the job progresses)
                  const cur = latestByJob.get(ev.jobId) ?? { status: ev.status, latencyMs: ev.latencyMs }
                  return (
                    <tr
                      key={ev.id}
                      className="border-b border-border last:border-0 hover:bg-muted/30 transition-colors"
                    >
                      {/* Status dot — reflects current status, not arrival status */}
                      <td className="py-2 pl-4 w-5">
                        <span
                          className="h-2 w-2 rounded-full inline-block"
                          style={{ background: statusDotColor(cur.status) }}
                        />
                      </td>
                      {/* Model */}
                      <td className="py-2 px-2 font-mono max-w-[160px] truncate text-foreground">
                        {ev.model}
                      </td>
                      {/* Provider */}
                      <td className="py-2 px-2 text-muted-foreground">
                        {ev.provider}
                      </td>
                      {/* Current status — color + icon + text (WCAG 1.4.1) */}
                      <td className="py-2 px-2" style={{ color: statusDotColor(cur.status) }}>
                        <span className="flex items-center gap-1">
                          <StatusIcon status={cur.status} />
                          {t(`jobs.statuses.${cur.status}` as Parameters<typeof t>[0])}
                        </span>
                      </td>
                      {/* Latency — populated once job completes */}
                      <td className="py-2 px-2 tabular-nums text-muted-foreground text-right">
                        {fmtMsNullable(cur.latencyMs)}
                      </td>
                      {/* Time ago (arrival time) */}
                      <td className="py-2 pl-2 pr-4 text-muted-foreground text-right whitespace-nowrap">
                        {fmtTimeAgo(ev.ts, t)}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  )
})
