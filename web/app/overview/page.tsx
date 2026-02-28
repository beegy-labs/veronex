'use client'

import Link from 'next/link'
import { useQuery, useQueries } from '@tanstack/react-query'
import { api } from '@/lib/api'
import type { Backend, Job, ModelBreakdown } from '@/lib/types'
import StatsCard from '@/components/stats-card'
import {
  Activity, CheckCircle, Zap, Sparkles, ArrowRight, ListFilter,
  Cpu, DollarSign, BarChart2,
} from 'lucide-react'
import {
  AreaChart, Area, BarChart, Bar, Cell,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtMs, fmtMsNullable,
} from '@/lib/chart-theme'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { useTranslation } from '@/i18n'

/* ─── OllamaIcon (inline) ─────────────────────────────────── */
function OllamaIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor"
      xmlns="http://www.w3.org/2000/svg" aria-label="Ollama">
      <path d="M7.5 1.5 C7 1.5 6.5 2 6.5 2.5 L6.5 5 C6.5 5.5 7 6 7.5 6 L9 6 C9.5 6 10 5.5 10 5 L10 2.5 C10 2 9.5 1.5 9 1.5 Z" />
      <path d="M15 1.5 C14.5 1.5 14 2 14 2.5 L14 5 C14 5.5 14.5 6 15 6 L16.5 6 C17 6 17.5 5.5 17.5 5 L17.5 2.5 C17.5 2 17 1.5 16.5 1.5 Z" />
      <ellipse cx="12" cy="9" rx="5.5" ry="4.5" />
      <path d="M9.5 13 L9.5 16 C9.5 16.5 10 17 10.5 17 L13.5 17 C14 17 14.5 16.5 14.5 16 L14.5 13 Z" />
      <rect x="6.5" y="16.5" width="11" height="6" rx="3" />
    </svg>
  )
}

/* ─── helpers ─────────────────────────────────────────────── */
function fmt(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

function fmtHour(iso: string) {
  const d = new Date(iso)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}h`
}

function fmtDate(iso: string) {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit',
  })
}

// fmtMsNullable imported from chart-theme
const fmtDuration = fmtMsNullable

function countByStatus(backends: Backend[], status: string) {
  return backends.filter(b => b.status === status).length
}

const STATUS_EXTRA: Record<string, string> = {
  completed: 'bg-status-success/15 text-status-success-fg border-status-success/30',
  failed:    'bg-status-error/15 text-status-error-fg border-status-error/30',
  cancelled: 'bg-status-cancelled/15 text-muted-foreground border-status-cancelled/30',
  pending:   'bg-status-warning/15 text-status-warning-fg border-status-warning/30',
  running:   'bg-status-info/15 text-status-info-fg border-status-info/30',
}

const ELECTRICITY_RATE = 0.10 // $/kWh

/* ─── skeleton ────────────────────────────────────────────── */
function StatSkeleton() {
  return (
    <Card>
      <CardContent className="p-5">
        <div className="h-3 w-24 rounded bg-muted animate-pulse mb-4" />
        <div className="h-8 w-16 rounded bg-muted animate-pulse mb-2" />
        <div className="h-2 w-20 rounded bg-muted animate-pulse" />
      </CardContent>
    </Card>
  )
}

/* ─── provider status row ─────────────────────────────────── */
function ProviderRow({
  icon, label, backends,
}: {
  icon: React.ReactNode
  label: string
  backends: Backend[]
}) {
  const online   = countByStatus(backends, 'online')
  const degraded = countByStatus(backends, 'degraded')
  const offline  = countByStatus(backends, 'offline')

  return (
    <div className="flex items-center justify-between py-2">
      <div className="flex items-center gap-2 text-sm font-medium">
        {icon}
        <span>{label}</span>
        <span className="text-muted-foreground text-xs">({backends.length})</span>
      </div>
      <div className="flex items-center gap-3 text-xs">
        {online > 0 && (
          <span className="flex items-center gap-1 text-status-success-fg">
            <span className="h-1.5 w-1.5 rounded-full bg-status-success inline-block" />
            {online}
          </span>
        )}
        {degraded > 0 && (
          <span className="flex items-center gap-1 text-status-warning-fg">
            <span className="h-1.5 w-1.5 rounded-full bg-status-warning inline-block" />
            {degraded}
          </span>
        )}
        {offline > 0 && (
          <span className="flex items-center gap-1 text-muted-foreground">
            <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground inline-block" />
            {offline}
          </span>
        )}
        {backends.length === 0 && (
          <span className="text-muted-foreground">—</span>
        )}
      </div>
    </div>
  )
}

/* ─── page ────────────────────────────────────────────────── */
export default function OverviewPage() {
  const { t } = useTranslation()

  const { data: stats, isLoading: statsLoading, error: statsError } = useQuery({
    queryKey: ['dashboard-stats'],
    queryFn: () => api.stats(),
    refetchInterval: 30_000,
  })

  const { data: backends } = useQuery({
    queryKey: ['backends'],
    queryFn: () => api.backends(),
    refetchInterval: 30_000,
  })

  const { data: servers } = useQuery({
    queryKey: ['servers'],
    queryFn: () => api.servers(),
    refetchInterval: 60_000,
    retry: false,
  })

  // Per-server live metrics (parallel queries, fail-open)
  const serverMetricQueries = useQueries({
    queries: (servers ?? []).map(s => ({
      queryKey: ['server-metrics', s.id],
      queryFn: () => api.serverMetrics(s.id),
      refetchInterval: 30_000,
      retry: false,
    })),
  })

  // ClickHouse-backed — graceful degradation when offline
  const { data: perf } = useQuery({
    queryKey: ['performance', 24],
    queryFn: () => api.performance(24),
    refetchInterval: 60_000,
    retry: false,
  })

  const { data: usage } = useQuery({
    queryKey: ['usage-aggregate', 24],
    queryFn: () => api.usageAggregate(24),
    refetchInterval: 60_000,
    retry: false,
  })

  const { data: breakdown } = useQuery({
    queryKey: ['usage-breakdown', 24],
    queryFn: () => api.usageBreakdown(24),
    refetchInterval: 60_000,
    retry: false,
  })

  const { data: recentJobsData } = useQuery({
    queryKey: ['recent-jobs'],
    queryFn: () => api.jobs('limit=10'),
    refetchInterval: 30_000,
  })

  /* ── derived: providers ────────────────────────────────── */
  const ollamaBs  = backends?.filter(b => b.backend_type === 'ollama') ?? []
  const geminiBs  = backends?.filter(b => b.backend_type === 'gemini') ?? []
  const onlineAll = [...ollamaBs, ...geminiBs].filter(b => b.status === 'online').length
  const totalProv = backends?.length ?? 0

  const queueDepth =
    (stats?.jobs_by_status['pending'] ?? 0) +
    (stats?.jobs_by_status['running'] ?? 0)

  const requests24h = perf?.total_requests ?? stats?.jobs_last_24h

  /* ── derived: power & cost ─────────────────────────────── */
  const totalPowerW = serverMetricQueries.reduce((sum, q) => {
    if (!q.data?.scrape_ok) return sum
    return sum + q.data.gpus.reduce((gs, g) => gs + (g.power_w ?? 0), 0)
  }, 0)
  const hasPowerData = totalPowerW > 0
  const kwhPerDay   = totalPowerW * 24 / 1000
  const dailyCost   = kwhPerDay * ELECTRICITY_RATE
  const efficiency  = kwhPerDay > 0 && (requests24h ?? 0) > 0
    ? Math.round((requests24h ?? 0) / kwhPerDay)
    : null

  /* ── derived: charts ───────────────────────────────────── */
  const trendData = perf?.hourly.map((h) => ({
    hour:    fmtHour(h.hour),
    total:   h.request_count,
    success: h.success_count,
  })) ?? []

  // Top 8 (model, provider) pairs by request count — from usageBreakdown
  const modelBarData: (ModelBreakdown & { label: string })[] = (breakdown?.by_model ?? [])
    .slice()
    .sort((a, b) => b.request_count - a.request_count)
    .slice(0, 8)
    .map(m => ({
      ...m,
      label: m.model_name.length > 22 ? m.model_name.slice(0, 21) + '…' : m.model_name,
    }))

  const recentJobs: Job[] = recentJobsData?.jobs ?? []

  /* ── error state ─────────────────────────────────────────── */
  if (statsError) {
    return (
      <Card className="border-destructive/50 bg-destructive/10">
        <CardContent className="p-6 text-destructive">
          <p className="font-semibold">{t('overview.failedStats')}</p>
          <p className="text-sm mt-1 opacity-80">
            {statsError instanceof Error ? statsError.message : t('common.unknownError')}
          </p>
        </CardContent>
      </Card>
    )
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t('overview.title')}</h1>
        <p className="text-muted-foreground mt-1 text-sm">
          {t('overview.description')}
        </p>
      </div>

      {/* ── Section 1: System KPIs ────────────────────────────── */}
      <div className="grid grid-cols-2 sm:grid-cols-3 xl:grid-cols-5 gap-4">
        {statsLoading ? (
          Array.from({ length: 5 }).map((_, i) => <StatSkeleton key={i} />)
        ) : (
          <>
            <StatsCard
              title={t('overview.providerStatus')}
              value={backends ? `${onlineAll}/${totalProv}` : '—'}
              subtitle={t('common.online')}
              icon={<Activity className="h-5 w-5" />}
            />
            <StatsCard
              title={t('overview.queueDepth')}
              value={stats ? queueDepth : '—'}
              subtitle={stats
                ? `${stats.jobs_by_status['pending'] ?? 0} pending · ${stats.jobs_by_status['running'] ?? 0} running`
                : undefined}
              icon={<ListFilter className="h-5 w-5" />}
            />
            <StatsCard
              title={t('overview.jobs24h')}
              value={requests24h != null ? fmt(requests24h) : '—'}
              subtitle={perf ? t('overview.requests') : t('overview.analyticsOffline')}
              icon={<Activity className="h-5 w-5" />}
            />
            <StatsCard
              title={t('performance.successRate')}
              value={perf ? `${Math.round(perf.success_rate * 100)}%` : '—'}
              subtitle={perf ? `${fmt(perf.total_requests)} ${t('overview.requests')}` : t('overview.analyticsOffline')}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title="P95"
              value={perf ? fmtMs(perf.p95_latency_ms) : '—'}
              subtitle={perf
                ? `P50 ${fmtMs(perf.p50_latency_ms)} · P99 ${fmtMs(perf.p99_latency_ms)}`
                : t('overview.analyticsOffline')}
              icon={<Zap className="h-5 w-5" />}
            />
          </>
        )}
      </div>

      {/* ── Section 2: Infrastructure (power / cost / efficiency) ── */}
      <div>
        <h2 className="text-xs font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
          {t('overview.infrastructure')}
        </h2>
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <StatsCard
            title={t('overview.gpuPower')}
            value={hasPowerData ? `${(totalPowerW / 1000).toFixed(2)} kW` : '—'}
            subtitle={hasPowerData
              ? `${(servers?.length ?? 0)} server${(servers?.length ?? 0) !== 1 ? 's' : ''}`
              : t('overview.noServerPower')}
            icon={<Cpu className="h-5 w-5" />}
          />
          <StatsCard
            title={t('overview.dailyElectricity')}
            value={hasPowerData ? `$${dailyCost.toFixed(2)}` : '—'}
            subtitle={hasPowerData ? t('overview.powerNote') : t('overview.noServerPower')}
            icon={<DollarSign className="h-5 w-5" />}
          />
          <StatsCard
            title={t('overview.efficiencyLabel')}
            value={efficiency != null ? `${fmt(efficiency)}` : '—'}
            subtitle={efficiency != null ? t('overview.reqPerKwh') : t('overview.noServerPower')}
            icon={<BarChart2 className="h-5 w-5" />}
          />
        </div>
      </div>

      {/* ── Section 3: Provider Status + API Keys ─────────────── */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Provider Status */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">{t('overview.providerStatus')}</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            <div className="divide-y divide-border">
              <ProviderRow
                icon={<OllamaIcon className="h-4 w-4" />}
                label="Ollama"
                backends={ollamaBs}
              />
              <ProviderRow
                icon={<Sparkles className="h-4 w-4" />}
                label="Gemini"
                backends={geminiBs}
              />
            </div>
            <div className="mt-3 pt-2 border-t border-border">
              <Link
                href="/providers"
                className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
              >
                {t('overview.goToProviders')} <ArrowRight className="h-3 w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>

        {/* API Keys */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">{t('keys.title')}</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            {statsLoading ? (
              <div className="h-12 rounded bg-muted animate-pulse" />
            ) : stats ? (
              <>
                <p className="text-3xl font-bold tabular-nums">{stats.active_keys}</p>
                <p className="text-xs text-muted-foreground mt-1">
                  {t('overview.activeKeys')} · {t('overview.totalKeysSubtitle', { count: stats.total_keys })}
                </p>
              </>
            ) : null}
            <div className="mt-3 pt-2 border-t border-border">
              <Link
                href="/keys"
                className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
              >
                {t('overview.goToKeys')} <ArrowRight className="h-3 w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* ── Section 4: Request Trend ──────────────────────────── */}
      {trendData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>{t('overview.requestTrend')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('overview.last24h')}</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={trendData}>
                <defs>
                  <linearGradient id="gradTotal" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%"  stopColor="var(--theme-primary)" stopOpacity={0.25} />
                    <stop offset="95%" stopColor="var(--theme-primary)" stopOpacity={0} />
                  </linearGradient>
                  <linearGradient id="gradSuccess" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%"  stopColor="var(--theme-status-success)" stopOpacity={0.2} />
                    <stop offset="95%" stopColor="var(--theme-status-success)" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={35} />
                <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} />
                <Legend wrapperStyle={LEGEND_STYLE} />
                <Area type="monotone" dataKey="total" name={t('overview.totalReqs')}
                  stroke="var(--theme-primary)" fill="url(#gradTotal)" strokeWidth={2} dot={false} />
                <Area type="monotone" dataKey="success" name={t('overview.successReqs')}
                  stroke="var(--theme-status-success)" fill="url(#gradSuccess)" strokeWidth={2} dot={false} />
              </AreaChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}

      {/* ── Section 5: Top Models (by provider) ──────────────── */}
      {modelBarData.length > 0 && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between flex-wrap gap-2">
              <div>
                <CardTitle>{t('overview.topModels')}</CardTitle>
                <p className="text-xs text-muted-foreground mt-0.5">{t('overview.last24h')}</p>
              </div>
              <div className="flex items-center gap-3 text-xs text-muted-foreground">
                <span className="flex items-center gap-1.5">
                  <span className="h-2.5 w-2.5 rounded-sm inline-block" style={{ background: 'var(--theme-primary)' }} />
                  Ollama
                </span>
                <span className="flex items-center gap-1.5">
                  <span className="h-2.5 w-2.5 rounded-sm inline-block" style={{ background: 'var(--theme-status-info)' }} />
                  Gemini
                </span>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={Math.max(160, modelBarData.length * 36)}>
              <BarChart data={modelBarData} layout="vertical" margin={{ left: 8, right: 16 }}>
                <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={fmt} />
                <YAxis
                  type="category" dataKey="label" width={154}
                  tick={{ ...AXIS_TICK, fontSize: 10 }}
                  axisLine={false} tickLine={false}
                />
                <Tooltip
                  contentStyle={TOOLTIP_STYLE}
                  labelStyle={TOOLTIP_LABEL_STYLE}
                  itemStyle={TOOLTIP_ITEM_STYLE}
                  cursor={CURSOR_FILL}
                  formatter={(v: number, _name: string, props: { payload?: ModelBreakdown }) => [
                    `${fmt(v)} ${t('usage.reqCount')}`,
                    props.payload?.backend ?? '',
                  ]}
                />
                <Bar dataKey="request_count" radius={[0, 4, 4, 0]}>
                  {modelBarData.map((m, i) => (
                    <Cell
                      key={i}
                      fill={m.backend === 'gemini'
                        ? 'var(--theme-status-info)'
                        : 'var(--theme-primary)'}
                    />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}

      {/* ── Section 6: Recent Jobs ───────────────────────────── */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-3">
          <CardTitle className="text-base">{t('overview.recentJobs')}</CardTitle>
          <Link
            href="/jobs"
            className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
          >
            {t('overview.viewAllJobs')} <ArrowRight className="h-3 w-3" />
          </Link>
        </CardHeader>
        {recentJobs.length === 0 ? (
          <CardContent className="pb-6 text-center text-sm text-muted-foreground">
            {t('jobs.noJobs')}
          </CardContent>
        ) : (
          <div className="overflow-x-auto">
            <table style={{ minWidth: '560px' }} className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="h-11 px-4 pl-6 text-left text-xs font-medium text-muted-foreground">{t('jobs.model')}</th>
                  <th className="h-11 px-4 text-left text-xs font-medium text-muted-foreground">{t('jobs.backend')}</th>
                  <th className="h-11 px-4 text-left text-xs font-medium text-muted-foreground">{t('jobs.status')}</th>
                  <th className="h-11 px-4 text-left text-xs font-medium text-muted-foreground">{t('jobs.latency')}</th>
                  <th className="h-11 px-4 pr-6 text-left text-xs font-medium text-muted-foreground">{t('jobs.createdAt')}</th>
                </tr>
              </thead>
              <tbody>
                {recentJobs.map((job) => (
                  <tr key={job.id} className="border-b border-border last:border-0">
                    <td className="py-3 px-4 pl-6 font-mono text-xs max-w-[180px] truncate">{job.model_name}</td>
                    <td className="py-3 px-4 text-xs text-muted-foreground max-w-[120px] truncate">{job.backend}</td>
                    <td className="py-3 px-4">
                      <Badge
                        variant="outline"
                        className={`text-xs ${STATUS_EXTRA[job.status] ?? 'bg-muted/20 text-muted-foreground border-muted/30'}`}
                      >
                        {job.status}
                      </Badge>
                    </td>
                    <td className="py-3 px-4 text-xs tabular-nums">{fmtDuration(job.latency_ms)}</td>
                    <td className="py-3 px-4 pr-6 text-xs text-muted-foreground whitespace-nowrap">{fmtDate(job.created_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      {/* ── Section 7: Token Summary + Performance ────────────── */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Token Summary */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">{t('overview.tokenSummary')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('overview.last24h')}</p>
          </CardHeader>
          <CardContent className="pt-0">
            {usage ? (
              <>
                <p className="text-3xl font-bold tabular-nums flex items-baseline gap-1">
                  {fmt(usage.total_tokens)}
                  <span className="text-sm font-normal text-muted-foreground">tokens</span>
                </p>
                <p className="text-xs text-muted-foreground mt-1">
                  {t('usage.promptTokens')} {fmt(usage.prompt_tokens)} · {t('usage.completionTokens')} {fmt(usage.completion_tokens)}
                </p>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">{t('overview.analyticsOffline')}</p>
            )}
            <div className="mt-3 pt-2 border-t border-border">
              <Link
                href="/usage"
                className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
              >
                {t('overview.goToUsage')} <ArrowRight className="h-3 w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>

        {/* Performance Summary */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">{t('overview.perfSummary')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('overview.last24h')}</p>
          </CardHeader>
          <CardContent className="pt-0">
            {perf ? (
              <div className="grid grid-cols-3 gap-2">
                {[
                  { label: 'P50', value: fmtMs(perf.p50_latency_ms) },
                  { label: 'P95', value: fmtMs(perf.p95_latency_ms) },
                  { label: 'P99', value: fmtMs(perf.p99_latency_ms) },
                ].map(({ label, value }) => (
                  <div key={label} className="text-center p-2 rounded-md bg-muted/40">
                    <p className="text-xs text-muted-foreground">{label}</p>
                    <p className="text-base font-bold tabular-nums mt-0.5">{value}</p>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{t('overview.analyticsOffline')}</p>
            )}
            <div className="mt-3 pt-2 border-t border-border">
              <Link
                href="/performance"
                className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
              >
                {t('overview.goToPerformance')} <ArrowRight className="h-3 w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
