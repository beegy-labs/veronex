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
  AXIS_TICK, CURSOR_FILL,
  fmtMs, fmtMsNullable, fmtCompact,
} from '@/lib/chart-theme'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { useTimezone } from '@/components/timezone-provider'
import { fmtHourLabel } from '@/lib/date'
import { useLabSettings } from '@/components/lab-settings-provider'
import { PROVIDER_OLLAMA, PROVIDER_GEMINI } from '@/lib/constants'
import {
  RequestTrendSection, TopModelsSection, RecentJobsSection, TokenSummarySection,
} from './dashboard-lower-sections'

import {
  type ThermalLevel,
  successRateCls, providerValueCls, pendingValueCls, latencyColor,
  countByStatus,
  THERMAL_ROW_CLS, THERMAL_NAME_CLS,
  StatSkeleton, ProviderRow, ThermalBadge, ConnectionDot,
} from './dashboard-helpers'

/* ─── props ───────────────────────────────────────────────── */
interface Props {
  stats: DashboardStats | undefined
  statsLoading: boolean
  providers: Provider[] | undefined
  servers: GpuServer[] | undefined
  serverMetricQueries: Array<{ data: NodeMetrics | undefined }>
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
  serverMetricQueries, serverHistoryQueries,
  perf, perf7d, perf30d,
  usage, breakdown, recentJobsData,
}: Props) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false

  /* ── derived: providers ─────────────────────────────────── */
  const LOCAL_TYPES = [PROVIDER_OLLAMA] as const
  const localBs = providers?.filter(b => (LOCAL_TYPES as readonly string[]).includes(b.provider_type)) ?? []
  const apiBs   = geminiEnabled
    ? (providers?.filter(b => b.provider_type === PROVIDER_GEMINI) ?? [])
    : []
  // visibleBs = only providers that are currently shown (respects lab flags)
  const visibleBs = [...localBs, ...apiBs]
  const onlineAll = visibleBs.filter(b => b.status === 'online').length
  const totalProv = visibleBs.length

  /* ── derived: server health (all servers) ───────────────── */
  const serverStatus = (servers ?? []).map((s, i) => {
    const m = serverMetricQueries[i]?.data
    const connected = m?.scrape_ok === true
    const maxTemp = connected && (m!.gpus?.length ?? 0) > 0
      ? m!.gpus.reduce((max, g) => Math.max(max, g.temp_junction_c ?? g.temp_c ?? 0, g.temp_mem_c ?? 0), 0)
      : null
    const thermal: ThermalLevel = maxTemp == null ? 'unknown'
      : maxTemp >= 90 ? 'critical'
      : maxTemp >= 80 ? 'warning'
      : 'normal'
    return { id: s.id, name: s.name, connected, maxTemp, thermal }
  })

  // Server status counts
  const connectedCount   = serverStatus.filter(s => s.connected).length
  const unreachableCount = serverStatus.filter(s => !s.connected).length
  const normalCount   = serverStatus.filter(s => s.thermal === 'normal').length
  const warningCount  = serverStatus.filter(s => s.thermal === 'warning').length
  const criticalCount = serverStatus.filter(s => s.thermal === 'critical').length

  // Thermal alert — servers needing attention (≥80°C)
  const hotServers = serverStatus.filter(s => s.thermal === 'warning' || s.thermal === 'critical')
  const hasCritical = hotServers.some(s => s.thermal === 'critical')

  /* ── derived: power ─────────────────────────────────────── */
  const hasPowerData = serverMetricQueries.some(q =>
    q.data?.scrape_ok && (q.data.gpus ?? []).some(g => (g.power_w ?? 0) > 0)
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
  const trendData = perf?.hourly.map((h) => ({
    hour:    fmtHourLabel(h.hour, tz),
    total:   h.request_count,
    success: h.success_count,
  })) ?? []

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

  const recentJobs: Job[] = recentJobsData?.jobs ?? []

  /* ── render ─────────────────────────────────────────────── */
  return (
    <div className="space-y-6">

      {/* Section 1: System KPIs */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {statsLoading ? (
          Array.from({ length: 3 }).map((_, i) => <StatSkeleton key={i} />)
        ) : (
          <>
            <StatsCard
              title={t('overview.providerStatus')}
              value={providers ? `${onlineAll}/${totalProv}` : '—'}
              subtitle={t('common.online')}
              icon={<Activity className="h-5 w-5" />}
              valueClassName={providers ? providerValueCls(onlineAll, totalProv) : ''}
            />
            <StatsCard
              title={t('overview.waiting')}
              value={stats ? (stats.jobs_by_status['pending'] ?? 0) : '—'}
              subtitle={t('overview.pendingJobs')}
              icon={<Clock className="h-5 w-5" />}
              valueClassName={stats ? pendingValueCls(stats.jobs_by_status['pending'] ?? 0) : ''}
            />
            <StatsCard
              title={t('overview.running')}
              value={stats ? (stats.jobs_by_status['running'] ?? 0) : '—'}
              subtitle={t('overview.runningJobs')}
              icon={<Activity className="h-5 w-5" />}
              valueClassName={stats && (stats.jobs_by_status['running'] ?? 0) > 0 ? 'text-status-info-fg' : ''}
            />
          </>
        )}
      </div>

      {/* Thermal Alert banner — only when ≥1 server ≥80°C */}
      {hotServers.length > 0 && (
        <div className={`rounded-lg border px-4 py-3 ${hasCritical ? 'border-status-error/40 bg-status-error/5' : 'border-status-warning/40 bg-status-warning/5'}`}>
          <div className="flex items-center justify-between gap-3 flex-wrap">
            <div className="flex items-center gap-2">
              <Thermometer className={`h-4 w-4 flex-shrink-0 ${hasCritical ? 'text-status-error-fg' : 'text-status-warning-fg'}`} />
              <span className={`text-sm font-semibold ${hasCritical ? 'text-status-error-fg' : 'text-status-warning-fg'}`}>
                {t('overview.thermalAlert')}
              </span>
              <span className="text-xs text-muted-foreground">
                — {t('overview.thermalAlertDesc', { count: hotServers.length })}
              </span>
            </div>
            <Link
              href="/servers"
              className={`text-xs font-medium flex items-center gap-1 transition-colors ${hasCritical ? 'text-status-error-fg hover:text-status-error-fg/80' : 'text-status-warning-fg hover:text-status-warning-fg/80'}`}
            >
              {t('overview.checkServers')} <ArrowRight className="h-3 w-3" />
            </Link>
          </div>
          <div className="mt-2 flex flex-wrap gap-2">
            {hotServers.map(s => (
              <div
                key={s.id}
                className={`flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium border ${
                  s.thermal === 'critical'
                    ? 'bg-status-error/10 border-status-error/30 text-status-error-fg'
                    : 'bg-status-warning/10 border-status-warning/30 text-status-warning-fg'
                }`}
              >
                <Thermometer className="h-3 w-3 flex-shrink-0" />
                <span className="truncate max-w-[120px]">{s.name}</span>
                {s.maxTemp != null && <span className="tabular-nums font-bold">{s.maxTemp.toFixed(0)}°C</span>}
                <span className="opacity-70">
                  {s.thermal === 'critical' ? t('overview.tempCritical') : t('overview.tempWarning')}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Section 2: Infrastructure */}
      <div>
        <h2 className="text-xs font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
          {t('overview.infrastructure')}
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">

          {/* Server Health — per-server status list */}
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium flex items-center gap-2">
                <HardDrive className="h-4 w-4 text-muted-foreground" />
                {t('overview.serverHealth')}
                {serverStatus.length > 0 && (
                  <span className="text-xs text-muted-foreground font-normal">({serverStatus.length})</span>
                )}
              </CardTitle>
              {serverStatus.length > 0 && (
                <div className="flex flex-wrap gap-x-3 gap-y-1 mt-1">
                  {/* Connection counts */}
                  <span className="flex items-center gap-1 text-[11px] font-medium text-status-success-fg">
                    <span className="h-1.5 w-1.5 rounded-full bg-status-success inline-block" />
                    {connectedCount} {t('overview.connected')}
                  </span>
                  {unreachableCount > 0 && (
                    <span className="flex items-center gap-1 text-[11px] font-medium text-status-error-fg">
                      <span className="h-1.5 w-1.5 rounded-full bg-status-error inline-block" />
                      {unreachableCount} {t('overview.unreachable')}
                    </span>
                  )}
                  {/* Thermal counts — only show non-normal states + normal count */}
                  <span className="flex items-center gap-1 text-[11px] font-medium text-status-success-fg">
                    <CheckCircle2 className="h-3 w-3" />
                    {normalCount} {t('overview.tempNormal')}
                  </span>
                  {warningCount > 0 && (
                    <span className="flex items-center gap-1 text-[11px] font-medium text-status-warning-fg">
                      <AlertTriangle className="h-3 w-3" />
                      {warningCount} {t('overview.tempWarning')}
                    </span>
                  )}
                  {criticalCount > 0 && (
                    <span className="flex items-center gap-1 text-[11px] font-medium text-status-error-fg">
                      <XCircle className="h-3 w-3" />
                      {criticalCount} {t('overview.tempCritical')}
                    </span>
                  )}
                </div>
              )}
            </CardHeader>
            <CardContent className="pt-0">
              {serverStatus.length === 0 ? (
                <p className="text-xs text-muted-foreground py-3">{t('overview.noServers')}</p>
              ) : (
                <div className="space-y-1">
                  {serverStatus.map(s => (
                    <div key={s.id} className={`flex items-center justify-between py-2 px-2 gap-2 rounded-sm ${THERMAL_ROW_CLS[s.thermal]}`}>
                      <span className={`text-sm font-medium truncate min-w-0 ${THERMAL_NAME_CLS[s.thermal]}`}>{s.name}</span>
                      <div className="flex items-center gap-3 flex-shrink-0">
                        <ConnectionDot connected={s.connected} t={t} />
                        <ThermalBadge level={s.thermal} temp={s.maxTemp} t={t} />
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <div className="mt-3 pt-2 border-t border-border">
                <Link href="/servers" className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">
                  {t('overview.checkServers')} <ArrowRight className="h-3 w-3" />
                </Link>
              </div>
            </CardContent>
          </Card>

          {/* Power cards */}
          <div className="md:col-span-2 grid grid-cols-1 sm:grid-cols-3 gap-4">
            <StatsCard
              title={t('overview.dailyPower')}
              value={(hasPowerData || hasHistory) ? `${kwhToday.toFixed(2)} kWh` : '—'}
              icon={<Zap className="h-5 w-5" />}
              subtitleNode={dailyDelta != null ? (
                <span className={dailyDelta > 0 ? 'text-status-warning-fg' : 'text-status-success-fg'}>
                  {dailyDelta > 0 ? '+' : ''}{dailyDelta.toFixed(2)} kWh {t('overview.sameDayLastWeek')}
                </span>
              ) : (
                <span className="text-muted-foreground">
                  {hasHistory ? t('overview.sameDayLastWeek') : t('overview.noServerPower')}
                </span>
              )}
            />
            <StatsCard
              title={t('overview.weeklyPower')}
              value={kwhThisWeek != null ? `${kwhThisWeek.toFixed(2)} kWh` : '—'}
              icon={<Zap className="h-5 w-5" />}
              subtitleNode={weekDelta != null ? (
                <span className={weekDelta > 0 ? 'text-status-warning-fg' : weekDelta < 0 ? 'text-status-success-fg' : 'text-muted-foreground'}>
                  {weekDelta > 0 ? '+' : ''}{weekDelta.toFixed(2)} kWh {t('overview.prevWeek')}
                </span>
              ) : (
                <span className="text-muted-foreground">
                  {hasHistory && historySpanD < 7
                    ? t('overview.daysData', { n: historySpanD.toFixed(1) })
                    : t('overview.noServerPower')}
                </span>
              )}
            />
            <StatsCard
              title={t('overview.monthlyPower')}
              value={kwhThisMonth != null ? `${kwhThisMonth.toFixed(2)} kWh` : '—'}
              icon={<Zap className="h-5 w-5" />}
              subtitleNode={monthDelta != null ? (
                <span className={monthDelta > 0 ? 'text-status-warning-fg' : monthDelta < 0 ? 'text-status-success-fg' : 'text-muted-foreground'}>
                  {monthDelta > 0 ? '+' : ''}{monthDelta.toFixed(2)} kWh {t('overview.prevMonth')}
                </span>
              ) : (
                <span className="text-muted-foreground">
                  {hasHistory && historySpanD < 30
                    ? t('overview.daysData', { n: historySpanD.toFixed(1) })
                    : t('overview.noServerPower')}
                </span>
              )}
            />
          </div>
        </div>
      </div>

      {/* Section 3: Workload + Latency Monitor */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">

        {/* Workload — metric × time-period table */}
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">{t('overview.workload')}</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            <table className="w-full text-sm">
              <thead>
                <tr>
                  <th className="text-left text-xs text-muted-foreground font-medium pb-3 w-[38%]" />
                  <th className="text-right text-xs text-muted-foreground font-medium pb-3">{t('overview.daily')}</th>
                  <th className="text-right text-xs text-muted-foreground font-medium pb-3">{t('overview.weekly')}</th>
                  <th className="text-right text-xs text-muted-foreground font-medium pb-3">{t('overview.monthly')}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                <tr>
                  <td className="py-3 text-xs text-muted-foreground">{t('overview.requests')}</td>
                  <td className="py-3 text-right font-bold tabular-nums">{perf    ? fmtCompact(perf.total_requests)    : '—'}</td>
                  <td className="py-3 text-right font-bold tabular-nums">{perf7d  ? fmtCompact(perf7d.total_requests)  : '—'}</td>
                  <td className="py-3 text-right font-bold tabular-nums">{perf30d ? fmtCompact(perf30d.total_requests) : '—'}</td>
                </tr>
                <tr>
                  <td className="py-3 text-xs text-muted-foreground">{t('performance.successRate')}</td>
                  {([perf, perf7d, perf30d] as const).map((d, i) => (
                    <td key={i} className="py-3 text-right">
                      {d != null ? (
                        <span className={`inline-flex items-center justify-center rounded px-1.5 py-0.5 text-xs font-bold tabular-nums ${successRateCls(d.success_rate)}`}>
                          {Math.round(d.success_rate)}%
                        </span>
                      ) : '—'}
                    </td>
                  ))}
                </tr>
              </tbody>
            </table>
          </CardContent>
        </Card>

        {/* Latency Monitor — P50/P95/P99 × time-period table + mini chart */}
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">{t('overview.latencyMonitor')}</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            <table className="w-full text-sm">
              <thead>
                <tr>
                  <th className="text-left text-xs text-muted-foreground font-medium pb-3 w-[20%]" />
                  <th className="text-right text-xs text-muted-foreground font-medium pb-3">{t('overview.daily')}</th>
                  <th className="text-right text-xs text-muted-foreground font-medium pb-3">{t('overview.weekly')}</th>
                  <th className="text-right text-xs text-muted-foreground font-medium pb-3">{t('overview.monthly')}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {([
                  { name: 'P50', key: 'p50_latency_ms' as const, warnMs: 1000,  errMs: 3000 },
                  { name: 'P95', key: 'p95_latency_ms' as const, warnMs: 2000,  errMs: 5000 },
                  { name: 'P99', key: 'p99_latency_ms' as const, warnMs: 5000,  errMs: 10000 },
                ]).map(({ name, key, warnMs, errMs }) => (
                  <tr key={name}>
                    <td className="py-3 text-xs font-medium text-muted-foreground">{name}</td>
                    {([perf, perf7d, perf30d] as const).map((d, i) => (
                      <td key={i} className={`py-3 text-right font-bold tabular-nums ${latencyColor(d?.[key], warnMs, errMs)}`}>
                        {d?.[key] != null ? fmtMs(d[key]!) : '—'}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
            {/* Mini 24h avg latency sparkline */}
            {perf && perf.hourly.length > 0 && (
              <div className="mt-4 pt-3 border-t border-border">
                <p className="text-[11px] text-muted-foreground mb-2">{t('overview.daily')} — avg / hour</p>
                <ResponsiveContainer width="100%" height={64}>
                  <AreaChart data={perf.hourly.map(h => ({ hour: fmtHourLabel(h.hour, tz), ms: h.avg_latency_ms }))}>
                    <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                    <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={38} tickFormatter={v => `${v}ms`} />
                    <Tooltip
                      contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                      formatter={(v) => [fmtMs(Number(v)), 'Avg'] as [string, string]}
                    />
                    <Area type="monotone" dataKey="ms" stroke="var(--theme-status-warning)"
                      fill="var(--theme-status-warning)" fillOpacity={0.1} strokeWidth={1.5} dot={false} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Section 4: Provider Status + API Keys */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-base">{t('overview.providerStatus')}</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            <div className="divide-y divide-border">
              <ProviderRow icon={<Server className="h-4 w-4" />} label={t('overview.localProviders')} providers={localBs} />
              {geminiEnabled && (
                <ProviderRow icon={<Globe className="h-4 w-4" />} label={t('overview.apiProviders')} providers={apiBs} />
              )}
            </div>
            <div className="mt-3 pt-2 border-t border-border">
              <Link href="/providers" className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">
                {t('overview.goToProviders')} <ArrowRight className="h-3 w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>

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
                <p className="text-xs text-muted-foreground mt-0.5">{t('overview.activeKeysLabel')}</p>
                <p className="text-xs text-muted-foreground mt-1">
                  {t('overview.totalKeysSubtitle', { count: stats.total_keys })}
                </p>
              </>
            ) : null}
            <div className="mt-3 pt-2 border-t border-border">
              <Link href="/keys" className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">
                {t('overview.goToKeys')} <ArrowRight className="h-3 w-3" />
              </Link>
            </div>
          </CardContent>
        </Card>
      </div>

      <RequestTrendSection trendData={trendData} />
      <TopModelsSection modelBarData={modelBarData} geminiEnabled={geminiEnabled} />
      <RecentJobsSection recentJobs={recentJobs} tz={tz} />
      <TokenSummarySection usage={usage} />
    </div>
  )
}
