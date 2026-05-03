'use client'

import { useState, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { serviceHealthQuery, pipelineHealthQuery } from '@/lib/queries'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { SERVICE_STATUS_DOT } from '@/lib/constants'
import { fmtCompact } from '@/lib/chart-theme'
import { Database, Server, HardDrive, Activity, Search, ChevronDown, ChevronUp, AlertTriangle, Package } from 'lucide-react'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
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
  if (hasError) return 'vds-text-error'
  if (!isActive || (lastPollSecs !== null && lastPollSecs > 120)) return 'vds-text-warning'
  if (lag > 1000) return 'vds-text-error'
  if (lag > 100) return 'vds-text-warning'
  return 'vds-text-success'
}

const POD_STATUS_COLOR: Record<string, string> = {
  online: 'vds-bg-success-bg',
  offline: 'vds-bg-error',
  degraded: 'vds-bg-warning',
}

function PodGrid({ pods }: { pods: PodItem[] }) {
  return (
    <div className="vds-border-t-1 vds-border-subtle vds-max-h-48 vds-overflow-y-auto">
      {pods.map(pod => (
        <div
          key={pod.id}
          className="vds-flex vds-items-center vds-pl-10 vds-pr-4 vds-py-2 vds-border-b-1 vds-border-subtle last:border-0 vds-bg-muted/20"
        >
          <Package className="vds-h-3.5 vds-w-3.5 vds-text-dim vds-flex-shrink-0 vds-mr-2" />
          <span className="vds-flex-1 vds-font-mono vds-text-xs vds-text-dim vds-truncate">{pod.id}</span>
          <span className="vds-ml-4 vds-text-xs vds-text-dim vds-tabular-nums vds-flex-shrink-0">
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
    <div className={isLast ? '' : 'vds-border-b-1 vds-border-subtle'}>
      <button
        type="button"
        className="vds-w-full vds-flex vds-items-center vds-justify-between vds-px-4 vds-py-2.5 vds-hover:bg-hover/40 vds-transition-colors vds-focus:outline-none"
        onClick={onToggle}
      >
        <span className="vds-text-sm">{label}</span>
        <div className="vds-flex vds-items-center vds-gap-2 vds-text-xs vds-text-dim">
          <span className="vds-flex vds-items-center vds-gap-1">
            <span className="vds-inline-block vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success-bg vds-flex-shrink-0" />
            {online} / {pods.length}
          </span>
          {open ? <ChevronUp className="vds-h-3.5 vds-w-3.5" /> : <ChevronDown className="vds-h-3.5 vds-w-3.5" />}
        </div>
      </button>
      {open && pods.length > 0 && <PodGrid pods={pods} />}
    </div>
  )
}

export default function HealthPage() {
  usePageGuard('dashboard_view')
  const { t } = useTranslation()
  const { data, isLoading, error } = useQuery(serviceHealthQuery)
  const { data: pipeline, isLoading: pipelineLoading } = useQuery(pipelineHealthQuery)
  const [apiPodsOpen, setApiPodsOpen] = useState(false)
  const [agentPodsOpen, setAgentPodsOpen] = useState(false)
  const [, tick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => tick(n => n + 1), 15_000)
    return () => clearInterval(id)
  }, [])

  if (error) {
    return (
      <div className="vds-p-6">
        <p className="vds-text-error">{t('common.error')}</p>
      </div>
    )
  }

  const stale = data?.infrastructure.some(s => isStale(s.checked_at)) ?? false
  const apiPods = data?.api_pods ?? []
  const agentPods = data?.agent_pods ?? []

  return (
    <div className="vds-space-y-6">
      {/* Header */}
      <div className="vds-flex vds-items-center vds-justify-between">
        <div>
          <h1 className="vds-text-xl vds-font-600">{t('health.title')}</h1>
          <p className="vds-text-sm vds-text-dim">{t('health.description')}</p>
        </div>
        {stale && !isLoading && (
          <span className="vds-px-2 vds-py-1 vds-text-xs vds-font-500 vds-rounded-md vds-bg-warning/15 vds-text-warning vds-border-1 vds-border-warning/30">
            {t('health.stale')}
          </span>
        )}
      </div>

      {/* Infrastructure */}
      <section className="vds-rounded-lg vds-border-1 vds-border-subtle vds-bg-card">
        <div className="vds-px-4 vds-py-2.5 vds-border-b-1 vds-border-subtle">
          <h2 className="vds-text-xs vds-font-500 vds-text-dim vds-uppercase vds-tracking-wide">{t('health.infrastructure')}</h2>
        </div>
        {isLoading ? (
          <div className="vds-p-4 vds-space-y-2">
            {[0,1,2,3,4].map(i => <div key={`skel-infra-${i}`} className="vds-h-7 vds-rounded vds-bg-muted vds-animate-pulse" />)}
          </div>
        ) : (data?.infrastructure ?? []).length === 0 ? (
          <p className="vds-px-4 vds-py-3 vds-text-sm vds-text-dim">{t('health.noData')}</p>
        ) : (
          <Table className="vds-text-sm">
            <TableBody>
              {(data?.infrastructure ?? []).map(svc => {
                const Icon = SVC_ICONS[svc.name] ?? Server
                const staleRow = isStale(svc.checked_at)
                return (
                  <TableRow key={svc.name} className="vds-border-b-1 vds-border-subtle last:border-0">
                    <TableCell className="vds-py-2 vds-pl-4 vds-pr-2 vds-w-4">
                      <span className={SERVICE_STATUS_DOT[svc.status] ?? ''} />
                    </TableCell>
                    <TableCell className="vds-py-2 vds-pr-3 vds-w-6">
                      <Icon className="vds-h-3.5 vds-w-3.5 vds-text-dim" />
                    </TableCell>
                    <TableCell className="vds-py-2 vds-font-500 vds-text-sm vds-w-36">
                      {SVC_LABELS[svc.name] ?? svc.name}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-text-xs vds-text-dim vds-w-20">
                      {t(`health.${svc.status}`)}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-text-xs vds-text-dim vds-tabular-nums vds-w-16">
                      {svc.latency_ms != null ? `${svc.latency_ms}ms` : '—'}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-pr-4 vds-text-right vds-text-xs vds-text-dim">
                      {timeAgo(svc.checked_at)}
                      {staleRow && <span className="vds-ml-1 vds-text-warning">⚠</span>}
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        )}
      </section>

      {/* Pods */}
      <section className="vds-rounded-lg vds-border-1 vds-border-subtle vds-bg-card">
        <div className="vds-px-4 vds-py-2.5 vds-border-b-1 vds-border-subtle">
          <h2 className="vds-text-xs vds-font-500 vds-text-dim vds-uppercase vds-tracking-wide">{t('health.pods')}</h2>
        </div>
        {isLoading ? (
          <div className="vds-p-4 vds-space-y-2">
            {[0,1].map(i => <div key={`skel-pod-${i}`} className="vds-h-8 vds-rounded vds-bg-muted vds-animate-pulse" />)}
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
      <section className="vds-rounded-lg vds-border-1 vds-border-subtle vds-bg-card">
        <div className="vds-px-4 vds-py-2.5 vds-border-b-1 vds-border-subtle">
          <h2 className="vds-text-xs vds-font-500 vds-text-dim vds-uppercase vds-tracking-wide">{t('health.pipeline')}</h2>
        </div>
        {pipelineLoading ? (
          <div className="vds-p-4 vds-space-y-2">
            {[0,1].map(i => <div key={`skel-pod-${i}`} className="vds-h-8 vds-rounded vds-bg-muted vds-animate-pulse" />)}
          </div>
        ) : !pipeline?.available || (pipeline?.topics ?? []).length === 0 ? (
          <p className="vds-px-4 vds-py-3 vds-text-sm vds-text-dim">{t('health.pipelineUnavailable')}</p>
        ) : (
          <Table className="vds-text-sm">
            <TableHeader>
              <TableRow className="vds-border-b-1 vds-border-subtle">
                <TableHead className="vds-py-2 vds-pl-4 vds-w-4" />
                <TableHead className="vds-py-2 vds-text-left vds-text-xs vds-font-500 vds-text-dim">{t('health.topic')}</TableHead>
                <TableHead className="vds-py-2 vds-text-right vds-text-xs vds-font-500 vds-text-dim">{t('health.consumers')}</TableHead>
                <TableHead className="vds-py-2 vds-text-right vds-text-xs vds-font-500 vds-text-dim">{t('health.lag')}</TableHead>
                <TableHead className="vds-py-2 vds-text-right vds-text-xs vds-font-500 vds-text-dim">{t('health.tpm1m')}</TableHead>
                <TableHead className="vds-py-2 vds-text-right vds-text-xs vds-font-500 vds-text-dim">{t('health.tpm5m')}</TableHead>
                <TableHead className="vds-py-2 vds-pr-4 vds-text-right vds-text-xs vds-font-500 vds-text-dim">{t('health.lastPoll')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {(pipeline?.topics ?? []).map(tp => {
                const hasError = !!tp.last_error
                const color = lagColor(tp.lag, tp.is_active, tp.last_poll_secs, hasError)
                const statusDot = hasError
                  ? 'vds-inline-block vds-h-2 vds-w-2 vds-rounded-full vds-bg-error vds-flex-shrink-0'
                  : tp.is_active
                    ? 'vds-inline-block vds-h-2 vds-w-2 vds-rounded-full vds-bg-success-bg vds-flex-shrink-0'
                    : 'vds-inline-block vds-h-2 vds-w-2 vds-rounded-full vds-bg-warning vds-flex-shrink-0'
                const lastPollLabel = tp.last_poll_secs == null ? '—'
                  : tp.last_poll_secs < 60 ? `${tp.last_poll_secs}s ago`
                  : `${Math.floor(tp.last_poll_secs / 60)}m ago`
                return (
                  <TableRow key={tp.topic} className="vds-border-b-1 vds-border-subtle last:border-0">
                    <TableCell className="vds-py-2 vds-pl-4 vds-pr-2">
                      <span className={statusDot} />
                    </TableCell>
                    <TableCell className="vds-py-2">
                      <div className="vds-flex vds-items-center vds-gap-1.5">
                        <span className="vds-font-mono vds-text-xs">{tp.topic}</span>
                        {hasError && (
                          <span title={tp.last_error ?? ''}>
                            <AlertTriangle className="vds-h-3 vds-w-3 vds-text-error vds-flex-shrink-0" />
                          </span>
                        )}
                      </div>
                      <div className="vds-text-[10px] vds-text-dim/50 vds-tabular-nums">
                        {fmtCompact(tp.consumer_offset)} / {fmtCompact(tp.log_end_offset)}
                      </div>
                    </TableCell>
                    <TableCell className="vds-py-2 vds-text-right vds-tabular-nums vds-text-xs vds-text-dim">{tp.consumer_count}</TableCell>
                    <TableCell className={`vds-py-2 vds-text-right vds-tabular-nums vds-font-mono vds-text-xs vds-font-600 ${color}`}>{fmtCompact(tp.lag)}</TableCell>
                    <TableCell className="vds-py-2 vds-text-right vds-tabular-nums vds-text-xs vds-text-dim">
                      {tp.tpm_1m === 0 ? <span className="vds-text-dim/40">0</span> : fmtCompact(tp.tpm_1m)}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-text-right vds-tabular-nums vds-text-xs vds-text-dim">
                      {tp.tpm_5m === 0 ? <span className="vds-text-dim/40">0</span> : `${fmtCompact(Math.round(tp.tpm_5m / 5 * 10) / 10)}/m`}
                    </TableCell>
                    <TableCell className="vds-py-2 vds-pr-4 vds-text-right vds-text-xs vds-text-dim">{lastPollLabel}</TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        )}
      </section>
    </div>
  )
}
