'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { serviceHealthQuery, pipelineHealthQuery } from '@/lib/queries'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { SERVICE_STATUS_DOT, SERVICE_STATUS_TEXT, PROVIDER_STATUS_DOT } from '@/lib/constants'
import { Database, Server, HardDrive, Activity, Container, ChevronDown, ChevronUp, AlertTriangle } from 'lucide-react'
import type { PodItem, TopicPipelineStats } from '@/lib/types'

const SVC_ICONS: Record<string, React.ComponentType<{ className?: string }>> = {
  postgresql: Database,
  valkey: Server,
  clickhouse: Activity,
  s3: HardDrive,
}

const SVC_LABELS: Record<string, string> = {
  postgresql: 'PostgreSQL',
  valkey: 'Valkey',
  clickhouse: 'ClickHouse',
  s3: 'S3 / MinIO',
}

function timeAgo(ms: number | null): string {
  if (!ms) return '-'
  const secs = Math.max(0, Math.floor((Date.now() - ms) / 1000))
  if (secs < 60) return `${secs}s ago`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`
  return `${Math.floor(secs / 3600)}h ago`
}

function isStale(ms: number | null): boolean {
  if (!ms) return true
  return Date.now() - ms > 60_000
}

function PodSection({ title, pods, isLoading }: { title: string; pods: PodItem[]; isLoading: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)

  const onlineCount = pods.filter(p => p.status === 'online').length
  const totalCount = pods.length

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium">{title}</CardTitle>
          {!isLoading && totalCount > 0 && (
            <button
              type="button"
              onClick={() => setOpen(v => !v)}
              className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              <span className="flex items-center gap-1">
                <span className="inline-block h-2 w-2 rounded-full bg-status-ok shrink-0" />
                {onlineCount} / {totalCount}
              </span>
              {open ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
            </button>
          )}
        </div>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="h-4 w-32 rounded bg-muted animate-pulse" />
        ) : totalCount === 0 ? (
          <p className="text-sm text-muted-foreground">{t('health.noPods')}</p>
        ) : !open ? (
          <p className="text-sm text-muted-foreground">{t('health.podsOnline', { online: onlineCount, total: totalCount })}</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="py-1.5 text-left text-xs font-medium text-muted-foreground w-4" />
                <th className="py-1.5 text-left text-xs font-medium text-muted-foreground">ID</th>
                <th className="py-1.5 text-right text-xs font-medium text-muted-foreground">{t('health.lastSeen')}</th>
              </tr>
            </thead>
            <tbody>
              {pods.map(pod => (
                <tr key={pod.id} className="border-b border-border last:border-0">
                  <td className="py-2 pr-2">
                    <span className={PROVIDER_STATUS_DOT[pod.status] ?? ''} />
                  </td>
                  <td className="py-2">
                    <div className="flex items-center gap-2">
                      <Container className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                      <span className="font-mono text-xs" title={pod.id}>{pod.id}</span>
                    </div>
                  </td>
                  <td className="py-2 text-right text-xs text-muted-foreground">
                    {pod.last_heartbeat_ms ? timeAgo(pod.last_heartbeat_ms) : t(`common.${pod.status}`)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </CardContent>
    </Card>
  )
}

function lagColor(lag: number, isActive: boolean, lastPollSecs: number | null, hasError: boolean): string {
  if (hasError) return 'text-status-error-fg'
  if (!isActive || (lastPollSecs !== null && lastPollSecs > 120)) return 'text-status-warning-fg'
  if (lag > 1000) return 'text-status-error-fg'
  if (lag > 100) return 'text-status-warning-fg'
  return 'text-status-ok'
}

function PipelineSection({ topics, isLoading }: { topics: TopicPipelineStats[]; isLoading: boolean }) {
  const { t } = useTranslation()

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-sm font-medium">{t('health.pipeline')}</CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="space-y-2">
            {[0, 1].map(i => <div key={i} className="h-12 rounded bg-muted animate-pulse" />)}
          </div>
        ) : topics.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t('health.pipelineUnavailable')}</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="py-1.5 text-left text-xs font-medium text-muted-foreground w-4" />
                <th className="py-1.5 text-left text-xs font-medium text-muted-foreground">{t('health.topic')}</th>
                <th className="py-1.5 text-right text-xs font-medium text-muted-foreground">{t('health.lag')}</th>
                <th className="py-1.5 text-right text-xs font-medium text-muted-foreground">{t('health.tpm1m')}</th>
                <th className="py-1.5 text-right text-xs font-medium text-muted-foreground">{t('health.tpm5m')}</th>
                <th className="py-1.5 text-right text-xs font-medium text-muted-foreground">{t('health.lastPoll')}</th>
              </tr>
            </thead>
            <tbody>
              {topics.map(tp => {
                const hasError = !!tp.last_error
                const color = lagColor(tp.lag, tp.is_active, tp.last_poll_secs, hasError)
                const statusDot = hasError
                  ? 'inline-block h-2 w-2 rounded-full bg-status-error shrink-0'
                  : tp.lag === 0 && tp.is_active
                    ? 'inline-block h-2 w-2 rounded-full bg-status-ok shrink-0'
                    : 'inline-block h-2 w-2 rounded-full bg-status-warning shrink-0'
                const lastPollLabel = tp.last_poll_secs == null ? '—'
                  : tp.last_poll_secs < 60 ? `${tp.last_poll_secs}s ago`
                  : `${Math.floor(tp.last_poll_secs / 60)}m ago`
                return (
                  <tr key={tp.topic} className="border-b border-border last:border-0">
                    <td className="py-2.5 pr-2">
                      <span className={statusDot} />
                    </td>
                    <td className="py-2.5">
                      <div className="flex items-center gap-1.5">
                        <span className="font-mono text-xs">{tp.topic}</span>
                        {hasError && (
                          <AlertTriangle className="h-3 w-3 text-status-error-fg shrink-0" title={tp.last_error ?? ''} />
                        )}
                      </div>
                      <div className="text-[10px] text-muted-foreground/60 tabular-nums">
                        {tp.consumer_offset.toLocaleString()} / {tp.log_end_offset.toLocaleString()}
                      </div>
                    </td>
                    <td className={`py-2.5 text-right tabular-nums font-mono text-xs font-semibold ${color}`}>
                      {tp.lag.toLocaleString()}
                    </td>
                    <td className="py-2.5 text-right tabular-nums text-xs text-muted-foreground">
                      {tp.tpm_1m === 0 ? <span className="text-muted-foreground/40">0</span> : tp.tpm_1m.toLocaleString()}
                    </td>
                    <td className="py-2.5 text-right tabular-nums text-xs text-muted-foreground">
                      {tp.tpm_5m === 0 ? <span className="text-muted-foreground/40">0</span> : `${(tp.tpm_5m / 5).toFixed(1)}/m`}
                    </td>
                    <td className="py-2.5 text-right text-xs text-muted-foreground">
                      {lastPollLabel}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </CardContent>
    </Card>
  )
}

export default function HealthPage() {
  const { t } = useTranslation()
  const { data, isLoading, error } = useQuery(serviceHealthQuery)
  const { data: pipeline, isLoading: pipelineLoading } = useQuery(pipelineHealthQuery)

  if (error) {
    return (
      <div className="p-6">
        <p className="text-status-error-fg">{t('common.error')}</p>
      </div>
    )
  }

  const stale = data?.infrastructure.some(s => isStale(s.checked_at)) ?? false

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">{t('health.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('health.description')}</p>
        </div>
        {stale && !isLoading && (
          <span className="px-2 py-1 text-xs font-medium rounded-md bg-status-warning/15 text-status-warning-fg border border-status-warning/30">
            {t('health.stale')}
          </span>
        )}
      </div>

      {/* Infrastructure Services */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium">{t('health.infrastructure')}</CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              {[0,1,2,3].map(i => (
                <div key={i} className="h-20 rounded-md bg-muted animate-pulse" />
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              {(data?.infrastructure ?? []).map(svc => {
                const Icon = SVC_ICONS[svc.name] ?? Server
                return (
                  <div
                    key={svc.name}
                    className="flex flex-col gap-2 p-3 rounded-md border border-border bg-card"
                  >
                    <div className="flex items-center gap-2">
                      <span className={SERVICE_STATUS_DOT[svc.status] ?? ''} />
                      <Icon className="h-4 w-4 text-muted-foreground" />
                      <span className="text-sm font-medium">{SVC_LABELS[svc.name] ?? svc.name}</span>
                    </div>
                    <div className="flex items-center justify-between text-xs text-muted-foreground">
                      <span className={SERVICE_STATUS_TEXT[svc.status] ?? ''}>
                        {t(`health.${svc.status}`)}
                      </span>
                      {svc.latency_ms != null && (
                        <span>{svc.latency_ms}ms</span>
                      )}
                    </div>
                    <div className="text-[10px] text-muted-foreground/60">
                      {timeAgo(svc.checked_at)}
                      {isStale(svc.checked_at) && (
                        <span className="ml-1 text-status-warning-fg">⚠</span>
                      )}
                    </div>
                  </div>
                )
              })}
              {(data?.infrastructure ?? []).length === 0 && (
                <p className="col-span-4 text-sm text-muted-foreground">{t('health.noData')}</p>
              )}
            </div>
          )}
        </CardContent>
      </Card>

      <PodSection title={t('health.apiPods')} pods={data?.api_pods ?? []} isLoading={isLoading} />
      <PodSection title={t('health.agentPods')} pods={data?.agent_pods ?? []} isLoading={isLoading} />
      <PipelineSection
        topics={pipeline?.available ? (pipeline?.topics ?? []) : []}
        isLoading={pipelineLoading}
      />
    </div>
  )
}
