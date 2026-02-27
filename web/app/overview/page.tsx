'use client'

import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import StatsCard from '@/components/stats-card'
import { Activity, Key, Layers, Clock, CheckCircle, Zap } from 'lucide-react'
import {
  AreaChart, Area,
  BarChart, Bar,
  XAxis, YAxis,
  Tooltip, ResponsiveContainer,
  Cell, Legend,
} from 'recharts'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'

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

const STATUS_COLORS: Record<string, string> = {
  completed: 'var(--theme-status-success)',
  failed:    'var(--theme-status-error)',
  cancelled: 'var(--theme-text-secondary)',
  pending:   'var(--theme-status-warning)',
  running:   'var(--theme-status-info)',
}

const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '8px',
  color: 'var(--theme-text-primary)',
  fontSize: '12px',
}

/* ─── skeleton ────────────────────────────────────────────── */
function StatSkeleton() {
  return (
    <Card>
      <CardContent className="p-6">
        <div className="h-3 w-24 rounded bg-muted animate-pulse mb-4" />
        <div className="h-8 w-16 rounded bg-muted animate-pulse mb-2" />
        <div className="h-2 w-20 rounded bg-muted animate-pulse" />
      </CardContent>
    </Card>
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

  // parallel — graceful degradation if ClickHouse is offline
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

  /* status bar chart — fixed order so chart is consistent across refreshes */
  const STATUS_ORDER = ['pending', 'running', 'completed', 'failed', 'cancelled']
  const statusData = stats
    ? STATUS_ORDER
        .filter((s) => stats.jobs_by_status[s] != null)
        .map((s) => ({ status: s, count: stats.jobs_by_status[s] }))
    : []

  /* hourly trend from performance */
  const trendData = perf?.hourly.map((h) => ({
    hour:    fmtHour(h.hour),
    total:   h.request_count,
    success: h.success_count,
    errors:  Math.max(0, h.request_count - h.success_count),
    tokens:  h.total_tokens,
  })) ?? []

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
    <div className="space-y-8">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold tracking-tight">
          {t('overview.title')}
        </h1>
        <p className="text-muted-foreground mt-1 text-sm">{t('overview.description')}</p>
      </div>

      {/* ── KPI grid ─────────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-4">
        {statsLoading ? (
          Array.from({ length: 6 }).map((_, i) => <StatSkeleton key={i} />)
        ) : stats ? (
          <>
            <StatsCard
              title={t('overview.totalJobs')}
              value={fmt(stats.total_jobs)}
              icon={<Layers className="h-5 w-5" />}
            />
            <StatsCard
              title={t('overview.jobs24h')}
              value={fmt(stats.jobs_last_24h)}
              icon={<Clock className="h-5 w-5" />}
            />
            <StatsCard
              title={t('overview.activeKeys')}
              value={stats.active_keys}
              subtitle={t('overview.totalKeysSubtitle', { count: stats.total_keys })}
              icon={<Key className="h-5 w-5" />}
            />

            {/* ClickHouse-backed cards — show placeholder when unavailable */}
            <StatsCard
              title={t('performance.successRate')}
              value={perf ? `${Math.round(perf.success_rate * 100)}%` : '—'}
              subtitle={perf ? `${fmt(perf.total_requests)} ${t('overview.requests')}` : t('overview.analyticsOffline')}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title={t('overview.totalTokens24h')}
              value={perf ? fmt(perf.total_tokens) : '—'}
              subtitle={usage
                ? `${fmt(usage.prompt_tokens)} prompt · ${fmt(usage.completion_tokens)} completion`
                : t('overview.analyticsOffline')}
              icon={<Zap className="h-5 w-5" />}
            />
            <StatsCard
              title={t('jobs.statuses.completed')}
              value={stats.jobs_by_status['completed'] ?? 0}
              subtitle={t('overview.failedSubtitle', { count: stats.jobs_by_status['failed'] ?? 0 })}
              icon={<Activity className="h-5 w-5" />}
            />
          </>
        ) : null}
      </div>

      {/* ── Hourly request trend ─────────────────────────────── */}
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
                <XAxis
                  dataKey="hour"
                  tick={{ fill: 'var(--theme-text-secondary)', fontSize: 11 }}
                  axisLine={false} tickLine={false}
                />
                <YAxis
                  tick={{ fill: 'var(--theme-text-secondary)', fontSize: 11 }}
                  axisLine={false} tickLine={false} width={35}
                />
                <Tooltip contentStyle={TOOLTIP_STYLE} cursor={{ fill: 'var(--theme-bg-hover)' }} />
                <Legend wrapperStyle={{ fontSize: '12px', color: 'var(--theme-text-secondary)' }} />
                <Area
                  type="monotone" dataKey="total" name={t('overview.totalReqs')}
                  stroke="var(--theme-primary)" fill="url(#gradTotal)"
                  strokeWidth={2} dot={false}
                />
                <Area
                  type="monotone" dataKey="success" name={t('overview.successReqs')}
                  stroke="var(--theme-status-success)" fill="url(#gradSuccess)"
                  strokeWidth={2} dot={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}

      {/* ── Jobs by status ────────────────────────────────────── */}
      {statusData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>{t('overview.jobsByStatus')}</CardTitle>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={statusData} barCategoryGap="30%">
                <XAxis
                  dataKey="status"
                  tick={{ fill: 'var(--theme-text-secondary)', fontSize: 12 }}
                  axisLine={false} tickLine={false}
                />
                <YAxis
                  tick={{ fill: 'var(--theme-text-secondary)', fontSize: 12 }}
                  axisLine={false} tickLine={false} width={40}
                />
                <Tooltip contentStyle={TOOLTIP_STYLE} cursor={{ fill: 'var(--theme-bg-hover)' }} />
                <Bar dataKey="count" radius={[4, 4, 0, 0]}>
                  {statusData.map((entry) => (
                    <Cell key={entry.status} fill={STATUS_COLORS[entry.status] ?? 'var(--theme-primary)'} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
