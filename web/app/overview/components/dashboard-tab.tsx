'use client'

import { useMemo } from 'react'
import Link from 'next/link'
import type { Provider, GpuServer, DashboardStats, PerformanceStats, UsageAggregate, UsageBreakdown, Job, NodeMetrics, ServerMetricsPoint, ModelBreakdown } from '@/lib/types'
import StatsCard from '@/components/stats-card'
import {
  Activity, Zap, ArrowRight, Clock,
  Server, Globe, HardDrive, Thermometer,
  CheckCircle2, XCircle, AlertTriangle,
} from 'lucide-react'
import {
  AreaChart, Area,
  XAxis, YAxis, Tooltip, ResponsiveContainer,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK,
  fmtMs, fmtCompact, fmtTemp, fmtKwh,
} from '@/lib/chart-theme'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtHourLabel } from '@/lib/date'
import { useLabSettings } from '@/components/lab-settings-provider'
import { PROVIDER_GEMINI, GPU_TEMP_CRITICAL, GPU_TEMP_WARNING } from '@/lib/constants'
import { tokens } from '@/lib/design-tokens'
import { getOllamaProviders, getGeminiProviders, successRateCls } from '@/lib/utils'
import {
  RequestTrendSection, TopModelsSection, RecentJobsSection, TokenSummarySection,
} from './dashboard-lower-sections'

import {
  type ThermalLevel,
  providerValueCls, pendingValueCls, latencyColor,
  THERMAL_ROW_CLS, THERMAL_NAME_CLS,
  StatSkeleton, ProviderRow, ThermalLevelBadge, ConnectionDot,
} from './dashboard-helpers'
import { SectionLabel } from '@/components/section-label'

/* ─── constants ────────────────────────────────────────────── */
const LATENCY_THRESHOLDS = [
  { name: 'P50', key: 'p50_latency_ms' as const, warnMs: 1_000,  errMs:  3_000 },
  { name: 'P95', key: 'p95_latency_ms' as const, warnMs: 2_000,  errMs:  5_000 },
  { name: 'P99', key: 'p99_latency_ms' as const, warnMs: 5_000,  errMs: 10_000 },
]

/* ─── props ───────────────────────────────────────────────── */
interface Props {
  stats: DashboardStats | undefined
  statsLoading: boolean
  providers: Provider[] | undefined
  servers: GpuServer[] | undefined
  /** Batch metrics map: server_id → NodeMetrics (single request, replaces N individual queries). */
  serverMetricsBatch: Record<string, NodeMetrics>
  serverHistoryQueries: Array<{ data: ServerMetricsPoint[] | undefined }>
  perf: PerformanceStats | undefined    // 24 h
  perf7d: PerformanceStats | undefined  // 7 d
  perf30d: PerformanceStats | undefined // 30 d
  usage: UsageAggregate | undefined
  breakdown: UsageBreakdown | undefined
  recentJobsData: { jobs: Job[]; total: number } | undefined
}

/* ─── component ───────────────────────────────────────────── */
export function DashboardTab({
  stats, statsLoading,
  providers, servers,
  serverMetricsBatch, serverHistoryQueries,
  perf, perf7d, perf30d,
  usage, breakdown, recentJobsData,
}: Props) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  /* ── derived: providers ─────────────────────────────────── */
  const { localBs, apiBs, onlineAll, totalProv } = useMemo(() => {
    const localBs = getOllamaProviders(providers)
    const apiBs   = geminiEnabled ? getGeminiProviders(providers) : []
    const visibleBs = [...localBs, ...apiBs]
    return {
      localBs,
      apiBs,
      onlineAll: visibleBs.filter(b => b.status === 'online').length,
      totalProv: visibleBs.length,
    }
  }, [providers, geminiEnabled])

  /* ── derived: server health (all servers) ───────────────── */
  const serverStatus = useMemo(() =>
    (servers ?? []).map((s) => {
      const m = serverMetricsBatch[s.id]
      const connected = m?.scrape_ok === true
      const maxTemp = connected && (m?.gpus?.length ?? 0) > 0
        ? m?.gpus?.reduce((max, g) => Math.max(max, g.temp_junction_c ?? g.temp_c ?? 0, g.temp_mem_c ?? 0), 0) ?? null
        : null
      const thermal: ThermalLevel = maxTemp == null ? 'unknown'
        : maxTemp >= GPU_TEMP_CRITICAL ? 'critical'
        : maxTemp >= GPU_TEMP_WARNING ? 'warning'
        : 'normal'
      return { id: s.id, name: s.name, connected, maxTemp, thermal }
    }),
    [servers, serverMetricsBatch],
  )

  // Server status counts + thermal alert — derived from memoized serverStatus
  const { connectedCount, unreachableCount, normalCount, warningCount, criticalCount, hotServers, hasCritical } = useMemo(() => {
    const connectedCount   = serverStatus.filter(s => s.connected).length
    const unreachableCount = serverStatus.filter(s => !s.connected).length
    const normalCount   = serverStatus.filter(s => s.thermal === 'normal').length
    const warningCount  = serverStatus.filter(s => s.thermal === 'warning').length
    const criticalCount = serverStatus.filter(s => s.thermal === 'critical').length
    const hotServers = serverStatus.filter(s => s.thermal === 'warning' || s.thermal === 'critical')
    return { connectedCount, unreachableCount, normalCount, warningCount, criticalCount, hotServers, hasCritical: hotServers.some(s => s.thermal === 'critical') }
  }, [serverStatus])

  /* ── derived: power ─────────────────────────────────────── */
  const hasPowerData = Object.values(serverMetricsBatch).some(m =>
    m?.scrape_ok && (m.gpus ?? []).some(g => (g.power_w ?? 0) > 0)
  )

  function sumKwhInRange(startMs: number, endMs: number): number {
    let total = 0
    for (const q of serverHistoryQueries) {
      for (const p of q.data ?? []) {
        if (p.gpu_power_w == null) continue
        const ts = new Date(p.ts).getTime()
        if (ts >= startMs && ts < endMs) total += p.gpu_power_w / 1000
      }
    }
    return total
  }

  function sumKwhInWindow(fromHoursAgo: number, toHoursAgo: number): number {
    const now = Date.now()
    return sumKwhInRange(now - fromHoursAgo * 3_600_000, now - toHoursAgo * 3_600_000)
  }

  const hasHistory = serverHistoryQueries.some(q => (q.data?.length ?? 0) > 0)

  // Compute history span to surface "X days of data" when accumulating
  let historyMinTs = Infinity
  let historyMaxTs = -Infinity
  for (const q of serverHistoryQueries) {
    for (const p of q.data ?? []) {
      const ts = new Date(p.ts).getTime()
      if (ts < historyMinTs) historyMinTs = ts
      if (ts > historyMaxTs) historyMaxTs = ts
    }
  }
  const historySpanH = hasHistory ? (historyMaxTs - historyMinTs) / 3_600_000 : 0
  const historySpanD = historySpanH / 24

  // Daily Power: today (midnight → now) vs same weekday last week
  const midnightToday  = new Date().setHours(0, 0, 0, 0)
  const kwhToday       = sumKwhInRange(midnightToday, Date.now())
  const kwhSameDay7d   = sumKwhInRange(midnightToday - 7 * 86_400_000, midnightToday - 6 * 86_400_000)
  const dailyDelta     = hasHistory && kwhSameDay7d > 0 ? kwhToday - kwhSameDay7d : null

  const kwhThisWeek   = hasHistory ? sumKwhInWindow(168, 0)   : null
  const kwhLastWeek   = hasHistory ? sumKwhInWindow(336, 168) : null
  const weekDelta     = kwhLastWeek != null && kwhLastWeek > 0 ? (kwhThisWeek ?? 0) - kwhLastWeek : null

  const kwhThisMonth  = hasHistory ? sumKwhInWindow(720, 0)    : null
  const kwhLastMonth  = hasHistory ? sumKwhInWindow(1440, 720) : null
  const monthDelta    = kwhLastMonth != null && kwhLastMonth > 0 ? (kwhThisMonth ?? 0) - kwhLastMonth : null

  /* ── derived: charts ────────────────────────────────────── */
  const trendData = useMemo(() =>
    (perf?.hourly ?? []).map((h) => ({
      hour:    fmtHourLabel(h.hour, tz),
      total:   h.request_count,
      success: h.success_count,
    })),
    [perf?.hourly, tz],
  )

  const modelBarData = useMemo<(ModelBreakdown & { label: string })[]>(() =>
    (breakdown?.by_model ?? [])
      .filter(m => geminiEnabled || m.provider_type !== PROVIDER_GEMINI)
      .slice()
      .sort((a, b) => b.request_count - a.request_count)
      .slice(0, 8)
      .map(m => ({
        ...m,
        label: m.model_name.length > 22 ? m.model_name.slice(0, 21) + '…' : m.model_name,
      })),
    [breakdown?.by_model, geminiEnabled],
  )

  // recentJobs removed — dashboard shows stats/issues, not individual jobs
  const perfMap = { daily: perf, weekly: perf7d, monthly: perf30d }

  /* ── render ─────────────────────────────────────────────── */
  return (
    <div className="vds-space-y-6">

      {/* Section 1: System KPIs */}
      <div className="vds-grid vds-grid-cols-1 vds-sm:grid-cols-3 vds-gap-4">
        {statsLoading ? (
          Array.from({ length: 3 }).map((_, i) => <StatSkeleton key={`stat-${i}`} />)
        ) : (
          <>
            <StatsCard
              title={t('overview.providerStatus')}
              value={providers ? `${onlineAll}/${totalProv}` : '—'}
              subtitle={t('common.online')}
              icon={<Activity className="vds-h-5 vds-w-5" />}
              valueClassName={providers ? providerValueCls(onlineAll, totalProv) : ''}
            />
            <StatsCard
              title={t('overview.waiting')}
              value={stats ? (stats.jobs_by_status['pending'] ?? 0) : '—'}
              subtitle={t('overview.pendingJobs')}
              icon={<Clock className="vds-h-5 vds-w-5" />}
              valueClassName={stats ? pendingValueCls(stats.jobs_by_status['pending'] ?? 0) : ''}
            />
            <StatsCard
              title={t('overview.running')}
              value={stats ? (stats.jobs_by_status['running'] ?? 0) : '—'}
              subtitle={t('overview.runningJobs')}
              icon={<Activity className="vds-h-5 vds-w-5" />}
              valueClassName={stats && (stats.jobs_by_status['running'] ?? 0) > 0 ? 'vds-text-info' : ''}
            />
          </>
        )}
      </div>

      {/* Thermal Alert banner — only when ≥1 server ≥80°C */}
      {hotServers.length > 0 && (
        <div className={`vds-rounded-lg vds-border-1 vds-px-4 vds-py-3 ${hasCritical ? 'vds-border-error/40 vds-bg-error/5' : 'vds-border-warning/40 vds-bg-warning/5'}`}>
          <div className="vds-flex vds-items-center vds-justify-between vds-gap-3 vds-flex-wrap">
            <div className="vds-flex vds-items-center vds-gap-2">
              <Thermometer className={`vds-h-4 vds-w-4 vds-flex-shrink-0 ${hasCritical ? 'vds-text-error' : 'vds-text-warning'}`} />
              <span className={`vds-text-sm vds-font-600 ${hasCritical ? 'vds-text-error' : 'vds-text-warning'}`}>
                {t('overview.thermalAlert')}
              </span>
              <span className="vds-text-xs vds-text-dim">
                — {t('overview.thermalAlertDesc', { count: hotServers.length })}
              </span>
            </div>
            <Link
              href="/servers"
              className={`vds-text-xs vds-font-500 vds-flex vds-items-center vds-gap-1 vds-transition-colors ${hasCritical ? 'vds-text-error vds-hover:text-error/80' : 'vds-text-warning vds-hover:text-warning/80'}`}
            >
              {t('overview.checkServers')} <ArrowRight className="vds-h-3 vds-w-3" />
            </Link>
          </div>
          <div className="vds-mt-2 vds-flex vds-flex-wrap vds-gap-2">
            {hotServers.map(s => (
              <div
                key={s.id}
                className={`vds-flex vds-items-center vds-gap-1.5 vds-rounded-md vds-px-2 vds-py-1 vds-text-xs vds-font-500 vds-border-1 ${
 s.thermal === 'critical'
 ? 'vds-bg-error-bg/10 vds-border-error/30 vds-text-error'
 : 'vds-bg-warning-bg/10 vds-border-warning/30 vds-text-warning'
 }`}
              >
                <Thermometer className="vds-h-3 vds-w-3 vds-flex-shrink-0" />
                <span className="vds-truncate vds-max-w-[40%]">{s.name}</span>
                {s.maxTemp != null && <span className="vds-tabular-nums vds-font-700">{fmtTemp(s.maxTemp)}</span>}
                <span className="vds-opacity-70">
                  {s.thermal === 'critical' ? t('overview.tempCritical') : t('overview.tempWarning')}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Section 2: Infrastructure */}
      <div>
        <SectionLabel as="h2" className="vds-text-xs">
          {t('overview.infrastructure')}
        </SectionLabel>
        <div className="vds-grid vds-grid-cols-1 vds-md:grid-cols-3 vds-gap-4">

          {/* Server Health — per-server status list */}
          <Card>
            <CardHeader className="vds-pb-2">
              <CardTitle className="vds-text-sm vds-font-500 vds-flex vds-items-center vds-gap-2">
                <HardDrive className="vds-h-4 vds-w-4 vds-text-dim" />
                {t('overview.serverHealth')}
                {serverStatus.length > 0 && (
                  <span className="vds-text-xs vds-text-dim vds-font-400">({serverStatus.length})</span>
                )}
              </CardTitle>
              {serverStatus.length > 0 && (
                <div className="vds-flex vds-flex-wrap vds-gap-x-3 vds-gap-y-1 vds-mt-1">
                  {/* Connection counts */}
                  <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-success">
                    <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-inline-block" />
                    {connectedCount} {t('overview.connected')}
                  </span>
                  {unreachableCount > 0 && (
                    <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-error">
                      <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-error vds-inline-block" />
                      {unreachableCount} {t('overview.unreachable')}
                    </span>
                  )}
                  {/* Thermal counts — only show non-normal states + normal count */}
                  <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-success">
                    <CheckCircle2 className="vds-h-3 vds-w-3" />
                    {normalCount} {t('overview.tempNormal')}
                  </span>
                  {warningCount > 0 && (
                    <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-warning">
                      <AlertTriangle className="vds-h-3 vds-w-3" />
                      {warningCount} {t('overview.tempWarning')}
                    </span>
                  )}
                  {criticalCount > 0 && (
                    <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-error">
                      <XCircle className="vds-h-3 vds-w-3" />
                      {criticalCount} {t('overview.tempCritical')}
                    </span>
                  )}
                </div>
              )}
            </CardHeader>
            <CardContent className="vds-pt-0">
              {serverStatus.length === 0 ? (
                <p className="vds-text-xs vds-text-dim vds-py-3">{t('overview.noServers')}</p>
              ) : (() => {
                const abnormal = serverStatus.filter(s => !s.connected || s.thermal === 'warning' || s.thermal === 'critical')
                return abnormal.length === 0 ? (
                  <p className="vds-text-xs vds-text-success vds-py-3">{t('overview.allServersNormal')}</p>
                ) : (
                  <div className="vds-space-y-1">
                    {abnormal.slice(0, 5).map(s => (
                      <div key={s.id} className={`vds-flex vds-items-center vds-justify-between vds-py-2 vds-px-2 vds-gap-2 vds-rounded-sm ${THERMAL_ROW_CLS[s.thermal]}`}>
                        <span className={`vds-text-sm vds-font-500 vds-truncate vds-min-w-0 ${THERMAL_NAME_CLS[s.thermal]}`}>{s.name}</span>
                        <div className="vds-flex vds-items-center vds-gap-3 vds-flex-shrink-0 vds-whitespace-nowrap">
                          <ConnectionDot connected={s.connected} />
                          <ThermalLevelBadge level={s.thermal} temp={s.maxTemp} />
                        </div>
                      </div>
                    ))}
                    {abnormal.length > 5 && (
                      <p className="vds-text-xs vds-text-dim vds-text-center vds-py-1">
                        +{abnormal.length - 5} {t('overview.moreServers')}
                      </p>
                    )}
                  </div>
                )
              })()}
              <div className="vds-mt-3 vds-pt-2 vds-border-t-1 vds-border-subtle">
                <Link href="/servers" className="vds-text-xs vds-text-dim vds-hover:text-primary vds-flex vds-items-center vds-gap-1 vds-transition-colors">
                  {t('overview.checkServers')} <ArrowRight className="vds-h-3 vds-w-3" />
                </Link>
              </div>
            </CardContent>
          </Card>

          {/* Power cards */}
          <div className="vds-md:col-span-2 vds-grid vds-grid-cols-1 vds-sm:grid-cols-3 vds-gap-4">
            <StatsCard
              title={t('overview.dailyPower')}
              value={(hasPowerData || hasHistory) ? fmtKwh(kwhToday) : '—'}
              icon={<Zap className="vds-h-5 vds-w-5" />}
              subtitleNode={dailyDelta != null ? (
                <span className={dailyDelta > 0 ? 'vds-text-warning' : 'vds-text-success'}>
                  {dailyDelta > 0 ? '+' : ''}{fmtKwh(dailyDelta)} {t('overview.sameDayLastWeek')}
                </span>
              ) : (
                <span className="vds-text-dim">
                  {hasHistory ? t('overview.sameDayLastWeek') : t('overview.noServerPower')}
                </span>
              )}
            />
            <StatsCard
              title={t('overview.weeklyPower')}
              value={fmtKwh(kwhThisWeek)}
              icon={<Zap className="vds-h-5 vds-w-5" />}
              subtitleNode={weekDelta != null ? (
                <span className={weekDelta > 0 ? 'vds-text-warning' : weekDelta < 0 ? 'vds-text-success' : 'vds-text-dim'}>
                  {weekDelta > 0 ? '+' : ''}{fmtKwh(weekDelta)} {t('overview.prevWeek')}
                </span>
              ) : (
                <span className="vds-text-dim">
                  {hasHistory && historySpanD < 7
                    ? t('overview.daysData', { n: fmtCompact(historySpanD) })
                    : t('overview.noServerPower')}
                </span>
              )}
            />
            <StatsCard
              title={t('overview.monthlyPower')}
              value={fmtKwh(kwhThisMonth)}
              icon={<Zap className="vds-h-5 vds-w-5" />}
              subtitleNode={monthDelta != null ? (
                <span className={monthDelta > 0 ? 'vds-text-warning' : monthDelta < 0 ? 'vds-text-success' : 'vds-text-dim'}>
                  {monthDelta > 0 ? '+' : ''}{fmtKwh(monthDelta)} {t('overview.prevMonth')}
                </span>
              ) : (
                <span className="vds-text-dim">
                  {hasHistory && historySpanD < 30
                    ? t('overview.daysData', { n: fmtCompact(historySpanD) })
                    : t('overview.noServerPower')}
                </span>
              )}
            />
          </div>
        </div>
      </div>

      {/* Section 3: Workload + Latency Monitor */}
      <div className="vds-grid vds-grid-cols-1 vds-md:grid-cols-2 vds-gap-4">

        {/* Workload — metric × time-period table */}
        <Card>
          <CardHeader className="vds-pb-3">
            <CardTitle className="vds-text-base">{t('overview.workload')}</CardTitle>
          </CardHeader>
          <CardContent className="vds-pt-0">
            <Table className="vds-text-sm">
              <TableHeader>
                <TableRow>
                  <TableHead className="vds-text-left vds-text-xs vds-text-dim vds-font-500 vds-pb-3 vds-w-[38%]" />
                  <TableHead className="vds-text-right vds-text-xs vds-text-dim vds-font-500 vds-pb-3">{t('overview.daily')}</TableHead>
                  <TableHead className="vds-text-right vds-text-xs vds-text-dim vds-font-500 vds-pb-3">{t('overview.weekly')}</TableHead>
                  <TableHead className="vds-text-right vds-text-xs vds-text-dim vds-font-500 vds-pb-3">{t('overview.monthly')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody className="vds-divide-y vds-divide-border">
                <TableRow>
                  <TableCell className="vds-py-3 vds-text-xs vds-text-dim">{t('overview.requests')}</TableCell>
                  <TableCell className="vds-py-3 vds-text-right vds-font-700 vds-tabular-nums">{perf    ? fmtCompact(perf.total_requests)    : '—'}</TableCell>
                  <TableCell className="vds-py-3 vds-text-right vds-font-700 vds-tabular-nums">{perf7d  ? fmtCompact(perf7d.total_requests)  : '—'}</TableCell>
                  <TableCell className="vds-py-3 vds-text-right vds-font-700 vds-tabular-nums">{perf30d ? fmtCompact(perf30d.total_requests) : '—'}</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="vds-py-3 vds-text-xs vds-text-dim">{t('performance.successRate')}</TableCell>
                  {(['daily', 'weekly', 'monthly'] as const).map((period) => {
                    const d = perfMap[period]
                    return (
                      <TableCell key={period} className="vds-py-3 vds-text-right">
                        {d != null ? (
                          <span className={`vds-inline-flex vds-items-center vds-justify-center vds-rounded vds-px-1.5 vds-py-0.5 vds-text-xs vds-font-700 vds-tabular-nums ${successRateCls(d.success_rate)}`}>
                            {Math.round(d.success_rate)}%
                          </span>
                        ) : '—'}
                      </TableCell>
                    )
                  })}
                </TableRow>
              </TableBody>
            </Table>
          </CardContent>
        </Card>

        {/* Latency Monitor — P50/P95/P99 × time-period table + mini chart */}
        <Card>
          <CardHeader className="vds-pb-3">
            <CardTitle className="vds-text-base">{t('overview.latencyMonitor')}</CardTitle>
          </CardHeader>
          <CardContent className="vds-pt-0">
            <Table className="vds-text-sm">
              <TableHeader>
                <TableRow>
                  <TableHead className="vds-text-left vds-text-xs vds-text-dim vds-font-500 vds-pb-3 vds-w-[20%]" />
                  <TableHead className="vds-text-right vds-text-xs vds-text-dim vds-font-500 vds-pb-3">{t('overview.daily')}</TableHead>
                  <TableHead className="vds-text-right vds-text-xs vds-text-dim vds-font-500 vds-pb-3">{t('overview.weekly')}</TableHead>
                  <TableHead className="vds-text-right vds-text-xs vds-text-dim vds-font-500 vds-pb-3">{t('overview.monthly')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody className="vds-divide-y vds-divide-border">
                {LATENCY_THRESHOLDS.map(({ name, key, warnMs, errMs }) => (
                  <TableRow key={name}>
                    <TableCell className="vds-py-3 vds-text-xs vds-font-500 vds-text-dim">{name}</TableCell>
                    {(['daily', 'weekly', 'monthly'] as const).map((period) => {
                      const d = perfMap[period]
                      return (
                        <TableCell key={period} className={`vds-py-3 vds-text-right vds-font-700 vds-tabular-nums ${latencyColor(d?.[key], warnMs, errMs)}`}>
                          {d?.[key] != null ? fmtMs(d[key]) : '—'}
                        </TableCell>
                      )
                    })}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {/* Mini 24h avg latency sparkline */}
            {perf && perf.hourly.length > 0 && (
              <div className="vds-mt-4 vds-pt-3 vds-border-t-1 vds-border-subtle">
                <p className="vds-text-2xs vds-text-dim vds-mb-2">{t('overview.daily')} — {t('overview.latencyAvgPerHour')}</p>
                <ResponsiveContainer width="100%" height={64}>
                  <AreaChart data={perf.hourly.map(h => ({ hour: fmtHourLabel(h.hour, tz), ms: h.avg_latency_ms }))}>
                    <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                    <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={38} tickFormatter={v => `${v}ms`} />
                    <Tooltip
                      contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                      formatter={(v) => [fmtMs(Number(v)), t('performance.avgLatency')] as [string, string]}
                    />
                    <Area type="monotone" dataKey="ms" stroke={tokens.status.warning}
                      fill={tokens.status.warning} fillOpacity={0.1} strokeWidth={1.5} dot={false} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Section 4: Provider Status + API Keys */}
      <div className="vds-grid vds-grid-cols-1 vds-md:grid-cols-2 vds-gap-4">
        <Card>
          <CardHeader className="vds-pb-2">
            <CardTitle className="vds-text-base">{t('overview.providerStatus')}</CardTitle>
          </CardHeader>
          <CardContent className="vds-pt-0">
            <div className="vds-divide-y vds-divide-border">
              <ProviderRow Icon={Server} label={t('overview.localProviders')} providers={localBs} />
              {geminiEnabled && (
                <ProviderRow Icon={Globe} label={t('overview.apiProviders')} providers={apiBs} />
              )}
            </div>
            <div className="vds-mt-3 vds-pt-2 vds-border-t-1 vds-border-subtle">
              <Link href="/providers" className="vds-text-xs vds-text-dim vds-hover:text-primary vds-flex vds-items-center vds-gap-1 vds-transition-colors">
                {t('overview.goToProviders')} <ArrowRight className="vds-h-3 vds-w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="vds-pb-2">
            <CardTitle className="vds-text-base">{t('keys.title')}</CardTitle>
          </CardHeader>
          <CardContent className="vds-pt-0">
            {statsLoading ? (
              <div className="vds-h-12 vds-rounded vds-bg-muted vds-animate-pulse" aria-busy="true" />
            ) : stats ? (
              <>
                <p className="vds-text-3xl vds-font-700 vds-tabular-nums">{stats.active_keys}</p>
                <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('overview.activeKeysLabel')}</p>
                <p className="vds-text-xs vds-text-dim vds-mt-1">
                  {t('overview.totalKeysSubtitle', { count: stats.total_keys })}
                </p>
              </>
            ) : null}
            <div className="vds-mt-3 vds-pt-2 vds-border-t-1 vds-border-subtle">
              <Link href="/keys" className="vds-text-xs vds-text-dim vds-hover:text-primary vds-flex vds-items-center vds-gap-1 vds-transition-colors">
                {t('overview.goToKeys')} <ArrowRight className="vds-h-3 vds-w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>

      <RequestTrendSection trendData={trendData} />
      <TopModelsSection modelBarData={modelBarData} geminiEnabled={geminiEnabled} />
      <TokenSummarySection usage={usage} />
    </div>
  )
}
