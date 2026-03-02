'use client'

import { useMemo } from 'react'
import { useTranslation } from '@/i18n'
import type { FlowEvent } from '@/hooks/use-inference-stream'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { fmtMsNullable } from '@/lib/chart-theme'

/* ─── helpers ─────────────────────────────────────────────── */
function timeAgo(ts: number): string {
  const s = Math.floor((Date.now() - ts) / 1000)
  if (s < 5)  return 'just now'
  if (s < 60) return `${s}s ago`
  return `${Math.floor(s / 60)}m ago`
}

function statusDotColor(status: string): string {
  switch (status) {
    case 'completed': return 'var(--theme-status-success)'
    case 'failed':    return 'var(--theme-status-error)'
    case 'running':   return 'var(--theme-status-info)'
    case 'cancelled': return 'var(--theme-status-cancelled)'
    default:          return 'var(--theme-status-warning)'
  }
}

/* ─── component ───────────────────────────────────────────── */
interface Props {
  events: FlowEvent[]
}

export function LiveFeed({ events }: Props) {
  const { t } = useTranslation()

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
                style={{ background: 'var(--theme-status-success)' }}
              />
              live
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
                      <td className="py-2 px-2 text-muted-foreground capitalize">
                        {ev.provider}
                      </td>
                      {/* Current status */}
                      <td className="py-2 px-2" style={{ color: statusDotColor(cur.status) }}>
                        {cur.status}
                      </td>
                      {/* Latency — populated once job completes */}
                      <td className="py-2 px-2 tabular-nums text-muted-foreground text-right">
                        {fmtMsNullable(cur.latencyMs)}
                      </td>
                      {/* Time ago (arrival time) */}
                      <td className="py-2 pl-2 pr-4 text-muted-foreground text-right whitespace-nowrap">
                        {timeAgo(ev.ts)}
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
}
