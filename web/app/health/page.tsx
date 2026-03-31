'use client'

import { useQuery } from '@tanstack/react-query'
import { serviceHealthQuery } from '@/lib/queries'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { SERVICE_STATUS_DOT, SERVICE_STATUS_TEXT, PROVIDER_STATUS_DOT } from '@/lib/constants'
import { Database, Server, HardDrive, Activity, Container } from 'lucide-react'

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

function truncateId(id: string): string {
  if (id.length <= 12) return id
  return `${id.slice(0, 8)}…`
}

export default function HealthPage() {
  const { t } = useTranslation()
  const { data, isLoading, error } = useQuery(serviceHealthQuery)

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

      {/* API Pods */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium">{t('health.apiPods')}</CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              {[0,1].map(i => (
                <div key={i} className="h-16 rounded-md bg-muted animate-pulse" />
              ))}
            </div>
          ) : (data?.api_pods ?? []).length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('health.noPods')}</p>
          ) : (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              {data!.api_pods.map(pod => (
                <div
                  key={pod.id}
                  className="flex flex-col gap-1.5 p-3 rounded-md border border-border bg-card"
                >
                  <div className="flex items-center gap-2">
                    <span className={PROVIDER_STATUS_DOT[pod.status] ?? ''} />
                    <Container className="h-3.5 w-3.5 text-muted-foreground" />
                    <span className="text-sm font-mono" title={pod.id}>
                      {truncateId(pod.id)}
                    </span>
                  </div>
                  <div className="text-[10px] text-muted-foreground/60">
                    {pod.last_heartbeat_ms ? timeAgo(pod.last_heartbeat_ms) : t(`common.${pod.status}`)}
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Agent Pods */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium">{t('health.agentPods')}</CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              {[0].map(i => (
                <div key={i} className="h-16 rounded-md bg-muted animate-pulse" />
              ))}
            </div>
          ) : (data?.agent_pods ?? []).length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('health.noPods')}</p>
          ) : (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              {data!.agent_pods.map(pod => (
                <div
                  key={pod.id}
                  className="flex flex-col gap-1.5 p-3 rounded-md border border-border bg-card"
                >
                  <div className="flex items-center gap-2">
                    <span className={PROVIDER_STATUS_DOT[pod.status] ?? ''} />
                    <Container className="h-3.5 w-3.5 text-muted-foreground" />
                    <span className="text-sm font-mono">{pod.id}</span>
                  </div>
                  <div className="text-[10px] text-muted-foreground/60">
                    {t(`common.${pod.status}`)}
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
