'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { serviceHealthQuery, pipelineHealthQuery } from '@/lib/queries'
import { useTranslation } from '@/i18n'
import { SERVICE_STATUS_DOT } from '@/lib/constants'
import { fmtCompact } from '@/lib/chart-theme'
import { Database, Server, HardDrive, Activity, Search, ChevronDown, ChevronUp, AlertTriangle, Package } from 'lucide-react'
import type { PodItem, TopicPipelineStats } from '@/lib/types'

const SVC_ICONS: Record<string, React.ComponentType<{ className?: string }>> = {
  postgresql: Database,
  valkey: Server,
  clickhouse: Activity,
  s3: HardDrive,
  vespa: Search,
}

const SVC_LABELS: Record<string, string> = {
  postgresql: 'PostgreSQL',
  valkey: 'Valkey',
  clickhouse: 'ClickHouse',
  s3: 'S3 / MinIO',
  vespa: 'Vespa',
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

function lagColor(lag: number, isActive: boolean, lastPollSecs: number | null, hasError: boolean): string {
  if (hasError) return 'text-status-error-fg'
  if (!isActive || (lastPollSecs !== null && lastPollSecs > 120)) return 'text-status-warning-fg'
  if (lag > 1000) return 'text-status-error-fg'
  if (lag > 100) return 'text-status-warning-fg'
  return 'text-status-ok'
}

const POD_STATUS_COLOR: Record<string, string> = {
  online: 'bg-status-ok',
  offline: 'bg-status-error',
  degraded: 'bg-status-warning',
}

function PodGrid({ pods }: { pods: PodItem[] }) {
  return (
    <div className="border-t border-border max-h-48 overflow-y-auto">
      {pods.map(pod => (
        <div
          key={pod.id}
          className="flex items-center pl-10 pr-4 py-2 border-b border-border last:border-0 bg-muted/20"
        >
          <Package className="h-3.5 w-3.5 text-muted-foreground shrink-0 mr-2" />
          <span className="flex-1 font-mono text-xs text-muted-foreground truncate">{pod.id}</span>
          <span className="ml-4 text-xs text-muted-foreground tabular-nums shrink-0">
            {timeAgo(pod.last_heartbeat_ms ?? null)}
          </span>
        </div>
      ))}
    </div>
  )
}

function PodGroup({
  label,
  pods,
  open,
  onToggle,
  isLast,
}: {
  label: string
  pods: PodItem[]
  open: boolean
  onToggle: () => void
  isLast: boolean
}) {
  const online = pods.filter(p => p.status === 'online').length
  return (
    <div className={isLast ? '' : 'border-b border-border'}>
      <button
        type="button"
        className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-muted/40 transition-colors focus:outline-none"
        onClick={onToggle}
      >
        <span className="text-sm">{label}</span>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="flex items-center gap-1">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-status-ok shrink-0" />
            {online} / {pods.length}
          </span>
          {open ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
        </div>
      </button>
      {open && pods.length > 0 && <PodGrid pods={pods} />}
    </div>
  )
}

export default function HealthPage() {
  const { t } = useTranslation()
  const { data, isLoading, error } = useQuery(serviceHealthQuery)
  const { data: pipeline, isLoading: pipelineLoading } = useQuery(pipelineHealthQuery)
  const [apiPodsOpen, setApiPodsOpen] = useState(false)
  const [agentPodsOpen, setAgentPodsOpen] = useState(false)

  if (error) {
    return (
      <div className="p-6">
        <p className="text-status-error-fg">{t('common.error')}</p>
      </div>
    )
  }

  const stale = data?.infrastructure.some(s => isStale(s.checked_at)) ?? false
  const apiPods = data?.api_pods ?? []
  const agentPods = data?.agent_pods ?? []

  return (
    <div className="p-6 space-y-4 max-w-3xl">
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

      {/* Infrastructure */}
      <section className="rounded-lg border border-border bg-card">
        <div className="px-4 py-2.5 border-b border-border">
          <h2 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('health.infrastructure')}</h2>
        </div>
        {isLoading ? (
          <div className="p-4 space-y-2">
            {[0,1,2,3,4].map(i => <div key={i} className="h-7 rounded bg-muted animate-pulse" />)}
          </div>
        ) : (data?.infrastructure ?? []).length === 0 ? (
          <p className="px-4 py-3 text-sm text-muted-foreground">{t('health.noData')}</p>
        ) : (
          <table className="w-full text-sm">
            <tbody>
              {(data?.infrastructure ?? []).map(svc => {
                const Icon = SVC_ICONS[svc.name] ?? Server
                const staleRow = isStale(svc.checked_at)
                return (
                  <tr key={svc.name} className="border-b border-border last:border-0">
                    <td className="py-2 pl-4 pr-2 w-4">
                      <span className={SERVICE_STATUS_DOT[svc.status] ?? ''} />
                    </td>
                    <td className="py-2 pr-3 w-6">
                      <Icon className="h-3.5 w-3.5 text-muted-foreground" />
                    </td>
                    <td className="py-2 font-medium text-sm w-36">
                      {SVC_LABELS[svc.name] ?? svc.name}
                    </td>
                    <td className="py-2 text-xs text-muted-foreground w-20">
                      {t(`health.${svc.status}`)}
                    </td>
                    <td className="py-2 text-xs text-muted-foreground tabular-nums w-16">
                      {svc.latency_ms != null ? `${svc.latency_ms}ms` : '—'}
                    </td>
                    <td className="py-2 pr-4 text-right text-xs text-muted-foreground">
                      {timeAgo(svc.checked_at)}
                      {staleRow && <span className="ml-1 text-status-warning-fg">⚠</span>}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </section>

      {/* Pods */}
      <section className="rounded-lg border border-border bg-card">
        <div className="px-4 py-2.5 border-b border-border">
          <h2 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('health.pods')}</h2>
        </div>
        {isLoading ? (
          <div className="p-4 space-y-2">
            {[0,1].map(i => <div key={i} className="h-8 rounded bg-muted animate-pulse" />)}
          </div>
        ) : (
          <>
            <PodGroup
              label={t('health.apiPods')}
              pods={apiPods}
              open={apiPodsOpen}
              onToggle={() => setApiPodsOpen(v => !v)}
              isLast={false}
            />
            <PodGroup
              label={t('health.agentPods')}
              pods={agentPods}
              open={agentPodsOpen}
              onToggle={() => setAgentPodsOpen(v => !v)}
              isLast
            />
          </>
        )}
      </section>

      {/* Pipeline */}
      <section className="rounded-lg border border-border bg-card">
        <div className="px-4 py-2.5 border-b border-border">
          <h2 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('health.pipeline')}</h2>
        </div>
        {pipelineLoading ? (
          <div className="p-4 space-y-2">
            {[0,1].map(i => <div key={i} className="h-8 rounded bg-muted animate-pulse" />)}
          </div>
        ) : !pipeline?.available || (pipeline?.topics ?? []).length === 0 ? (
          <p className="px-4 py-3 text-sm text-muted-foreground">{t('health.pipelineUnavailable')}</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="py-2 pl-4 w-4" />
                <th className="py-2 text-left text-xs font-medium text-muted-foreground">{t('health.topic')}</th>
                <th className="py-2 text-right text-xs font-medium text-muted-foreground">{t('health.consumers')}</th>
                <th className="py-2 text-right text-xs font-medium text-muted-foreground">{t('health.lag')}</th>
                <th className="py-2 text-right text-xs font-medium text-muted-foreground">{t('health.tpm1m')}</th>
                <th className="py-2 text-right text-xs font-medium text-muted-foreground">{t('health.tpm5m')}</th>
                <th className="py-2 pr-4 text-right text-xs font-medium text-muted-foreground">{t('health.lastPoll')}</th>
              </tr>
            </thead>
            <tbody>
              {(pipeline?.topics ?? []).map(tp => {
                const hasError = !!tp.last_error
                const color = lagColor(tp.lag, tp.is_active, tp.last_poll_secs, hasError)
                const statusDot = hasError
                  ? 'inline-block h-2 w-2 rounded-full bg-status-error shrink-0'
                  : tp.is_active
                    ? 'inline-block h-2 w-2 rounded-full bg-status-ok shrink-0'
                    : 'inline-block h-2 w-2 rounded-full bg-status-warning shrink-0'
                const lastPollLabel = tp.last_poll_secs == null ? '—'
                  : tp.last_poll_secs < 60 ? `${tp.last_poll_secs}s ago`
                  : `${Math.floor(tp.last_poll_secs / 60)}m ago`
                return (
                  <tr key={tp.topic} className="border-b border-border last:border-0">
                    <td className="py-2 pl-4 pr-2">
                      <span className={statusDot} />
                    </td>
                    <td className="py-2">
                      <div className="flex items-center gap-1.5">
                        <span className="font-mono text-xs">{tp.topic}</span>
                        {hasError && (
                          <span title={tp.last_error ?? ''}>
                            <AlertTriangle className="h-3 w-3 text-status-error-fg shrink-0" />
                          </span>
                        )}
                      </div>
                      <div className="text-[10px] text-muted-foreground/50 tabular-nums">
                        {fmtCompact(tp.consumer_offset)} / {fmtCompact(tp.log_end_offset)}
                      </div>
                    </td>
                    <td className="py-2 text-right tabular-nums text-xs text-muted-foreground">{tp.consumer_count}</td>
                    <td className={`py-2 text-right tabular-nums font-mono text-xs font-semibold ${color}`}>{fmtCompact(tp.lag)}</td>
                    <td className="py-2 text-right tabular-nums text-xs text-muted-foreground">
                      {tp.tpm_1m === 0 ? <span className="text-muted-foreground/40">0</span> : fmtCompact(tp.tpm_1m)}
                    </td>
                    <td className="py-2 text-right tabular-nums text-xs text-muted-foreground">
                      {tp.tpm_5m === 0 ? <span className="text-muted-foreground/40">0</span> : `${fmtCompact(Math.round(tp.tpm_5m / 5 * 10) / 10)}/m`}
                    </td>
                    <td className="py-2 pr-4 text-right text-xs text-muted-foreground">{lastPollLabel}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </section>
    </div>
  )
}
