'use client'

import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTimeRange } from '@/components/time-range-context'
import { performanceQuery, usageBreakdownQuery, analyticsQuery } from '@/lib/queries'
import {
  LineChart, Line, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer,
  ReferenceLine, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL, CURSOR_STROKE,
  fmtMs, fmtMsAxis, fmtCompact, fmtPct, fmtTps,
} from '@/lib/chart-theme'
import { Timer, TrendingUp, CheckCircle, AlertTriangle, Zap } from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { TIME_LABEL_MAP, TimeRangeSelector } from '@/components/time-range-selector'
import { fmtHourLabel } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'
import { ModelLatencySection } from './components/model-latency-section'
import { KeyPerformanceSection } from './components/key-performance-section'
import { tokens } from '@/lib/design-tokens'

/* ─── page ────────────────────────────────────────────────── */
export default function PerformancePage() {
  usePageGuard('dashboard_view')
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const { range, setRange } = useTimeRange()
  const hours = range.hours

  const { data, isLoading, error } = useQuery(performanceQuery(hours))
  const { data: breakdown } = useQuery(usageBreakdownQuery(hours))
  const { data: analytics } = useQuery(analyticsQuery(hours))

  const chartData = useMemo(() =>
    (data?.hourly ?? []).map((h) => ({
      hour:      fmtHourLabel(h.hour, tz),
      latency:   Math.round(h.avg_latency_ms),
      total:     h.request_count,
      success:   h.success_count,
      errors:    Math.max(0, h.request_count - h.success_count),
      tokens:    h.total_tokens,
      errorRate: h.request_count > 0
        ? Math.round((h.request_count - h.success_count) / h.request_count * 1000) / 10
        : 0,
      tps: h.request_count > 0 && h.total_tokens > 0
        ? Math.round(h.total_tokens / (h.avg_latency_ms / 1000 * h.request_count) * 10) / 10
        : 0,
    })),
    [data?.hourly, tz],
  )

  const hasData    = data && data.total_requests > 0
  const errorCount = data ? data.total_requests - Math.round(data.success_rate / 100 * data.total_requests) : 0
  const currentLabel = TIME_LABEL_MAP.get(hours) ?? `${hours}h`

  // Merge analytics model stats (has success_rate) with breakdown model data (has avg_latency_ms)
  const modelPerfData = useMemo(() => {
    if (!breakdown?.by_model) return []
    const analyticsMap = new Map(
      (analytics?.models ?? []).map((m) => [m.model_name, m])
    )
    return breakdown.by_model
      .filter((m) => m.avg_latency_ms > 0)
      .map((m) => ({
        model_name:    m.model_name,
        provider_type: m.provider_type,
        request_count: m.request_count,
        avg_latency_ms: m.avg_latency_ms,
        success_rate:  analyticsMap.get(m.model_name)?.success_rate ?? undefined,
      }))
      .sort((a, b) => a.avg_latency_ms - b.avg_latency_ms)
  }, [breakdown?.by_model, analytics?.models])

  return (
    <div className="vds-space-y-6">
      {/* Header */}
      <div className="vds-flex vds-items-center vds-justify-between vds-flex-wrap vds-gap-4">
        <div>
          <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('performance.title')}</h1>
          <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('performance.description')}</p>
        </div>
        <TimeRangeSelector value={range} onChange={setRange} />
      </div>

      {/* ClickHouse unavailable */}
      {error && (
        <Card className="vds-border-warning/30 vds-bg-warning/10">
          <CardContent className="vds-p-5">
            <p className="vds-font-600 vds-text-warning">{t('performance.analyticsUnavailable')}</p>
            <p className="vds-text-sm vds-mt-1 vds-text-warning/80">{t('performance.clickhouseDisabled')}</p>
          </CardContent>
        </Card>
      )}

      {isLoading && (
        <div className="vds-grid vds-grid-cols-3 vds-sm:grid-cols-5 vds-gap-4">
          {Array.from({ length: 5 }).map((_, i) => (
            <Card key={i}><CardContent className="vds-p-6">
              <div className="vds-h-3 vds-w-24 vds-rounded vds-bg-muted vds-animate-pulse vds-mb-4" />
              <div className="vds-h-8 vds-w-16 vds-rounded vds-bg-muted vds-animate-pulse" />
            </CardContent></Card>
          ))}
        </div>
      )}

      {!error && !isLoading && !hasData && (
        <Card>
          <CardContent className="vds-p-10 vds-text-center vds-text-dim">
            <p className="vds-font-500">{t('performance.noData')}</p>
            <p className="vds-text-sm vds-mt-1">{t('performance.noDataHint')}</p>
          </CardContent>
        </Card>
      )}

      {!error && data && hasData && (
        <>
          {/* ── KPI cards (5) ───────────────────────────────── */}
          <div className="vds-grid vds-grid-cols-3 vds-sm:grid-cols-5 vds-gap-4">
            <StatsCard
              title={t('performance.p50')}
              value={fmtMs(data.p50_latency_ms)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<Timer className="vds-h-5 vds-w-5" />}
            />
            <StatsCard
              title={t('performance.p95')}
              value={fmtMs(data.p95_latency_ms)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<TrendingUp className="vds-h-5 vds-w-5" />}
            />
            <StatsCard
              title={t('performance.p99')}
              value={fmtMs(data.p99_latency_ms)}
              subtitle={`avg ${fmtMs(data.avg_latency_ms)}`}
              icon={<TrendingUp className="vds-h-5 vds-w-5" />}
            />
            <StatsCard
              title={t('performance.successRate')}
              value={fmtPct(data.success_rate)}
              subtitle={`${fmtCompact(data.total_requests)} ${t('overview.requests')}`}
              icon={<CheckCircle className="vds-h-5 vds-w-5" />}
            />
            <StatsCard
              title={t('performance.errors')}
              value={fmtCompact(errorCount)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<AlertTriangle className="vds-h-5 vds-w-5" style={errorCount > 0 ? { color: tokens.status.error } : undefined} />}
            />
          </div>

          {/* ── Analytics TPS card (if available) ──────── */}
          {analytics && (
            <div className="vds-grid vds-grid-cols-2 vds-sm:grid-cols-4 vds-gap-4">
              <StatsCard
                title={t('usage.avgTps')}
                value={fmtTps(analytics.avg_tps)}
                subtitle={t('usage.avgTpsDesc')}
                icon={<Zap className="vds-h-5 vds-w-5" />}
              />
              <StatsCard
                title={t('usage.avgPromptTokens')}
                value={analytics.avg_prompt_tokens > 0 ? fmtCompact(analytics.avg_prompt_tokens) : '—'}
                subtitle={t('usage.tokensPerReq')}
                icon={<Timer className="vds-h-5 vds-w-5" />}
              />
              <StatsCard
                title={t('usage.avgCompletionTokens')}
                value={analytics.avg_completion_tokens > 0 ? fmtCompact(analytics.avg_completion_tokens) : '—'}
                subtitle={t('usage.tokensPerReq')}
                icon={<Timer className="vds-h-5 vds-w-5" />}
              />
              <StatsCard
                title={t('performance.totalRequests')}
                value={fmtCompact(data.total_requests)}
                subtitle={`${t('common.last')} ${currentLabel}`}
                icon={<CheckCircle className="vds-h-5 vds-w-5" />}
              />
            </div>
          )}

          {/* ── Model performance breakdown ─────────────── */}
          {modelPerfData.length > 0 && (
            <ModelLatencySection models={modelPerfData} />
          )}

          {/* ── Per-key performance ─────────────────────── */}
          {breakdown && breakdown.by_key.length > 0 && (
            <KeyPerformanceSection keys={breakdown.by_key} />
          )}

          {chartData.length > 0 && (
            <>
              {/* ── Avg latency trend ─────────────────────── */}
              <Card>
                <CardHeader>
                  <CardTitle className="vds-text-base">{t('performance.avgLatencyHour')}</CardTitle>
                  <p className="vds-text-xs vds-text-dim">
                    {t('performance.p95ReferenceLine')}: {fmtMs(data.p95_latency_ms)}
                  </p>
                </CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={200}>
                    <LineChart data={chartData}>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={55} tickFormatter={fmtMsAxis} />
                      <Tooltip
                        contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                        cursor={CURSOR_STROKE}
                        formatter={(v) => [fmtMs(Number(v)), t('performance.avgLatency')] as [string, string]}
                      />
                      <ReferenceLine
                        y={data.p95_latency_ms}
                        stroke={tokens.status.warning}
                        strokeDasharray="4 4"
                        label={{ value: 'P95', position: 'right', fill: tokens.status.warning, fontSize: 11 }}
                      />
                      <Line
                        type="monotone" dataKey="latency"
                        stroke={tokens.brand.primary} strokeWidth={2} dot={false}
                      />
                    </LineChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              {/* ── Throughput: total / success / errors ─────── */}
              <Card>
                <CardHeader>
                  <div className="vds-flex vds-items-center vds-justify-between">
                    <CardTitle className="vds-text-base">{t('performance.throughputHour')}</CardTitle>
                    {errorCount > 0 && (
                      <span className="vds-flex vds-items-center vds-gap-1.5 vds-text-xs vds-text-error">
                        <AlertTriangle className="vds-h-3.5 vds-w-3.5" />
                        {fmtCompact(errorCount)} {t('performance.errors')}
                      </span>
                    )}
                  </div>
                </CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={200}>
                    <BarChart data={chartData} barGap={2}>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={35} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} />
                      <Legend wrapperStyle={LEGEND_STYLE} />
                      <Bar dataKey="total"   name={t('overview.totalReqs')}   fill={tokens.brand.primary}    radius={[3, 3, 0, 0]} />
                      <Bar dataKey="success" name={t('overview.successReqs')} fill={tokens.status.success} radius={[3, 3, 0, 0]} />
                      <Bar dataKey="errors"  name={t('performance.errors')}   fill={tokens.status.error}   radius={[3, 3, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              {/* ── TPS trend ──────────────────────────────────── */}
              {chartData.some((d) => d.tps > 0) && (
                <Card>
                  <CardHeader>
                    <CardTitle className="vds-text-base">{t('performance.tpsHour')}</CardTitle>
                    {analytics && analytics.avg_tps > 0 && (
                      <p className="vds-text-xs vds-text-dim">
                        {t('usage.avgTps')}: <span className="vds-font-600 vds-text-primary">{fmtTps(analytics.avg_tps)}</span>
                      </p>
                    )}
                  </CardHeader>
                  <CardContent>
                    <ResponsiveContainer width="100%" height={180}>
                      <LineChart data={chartData}>
                        <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                        <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={42} tickFormatter={(v) => `${v}`} />
                        <Tooltip
                          contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                          cursor={CURSOR_STROKE}
                          formatter={(v) => [fmtTps(Number(v)), t('usage.avgTps')] as [string, string]}
                        />
                        <Line
                          type="monotone" dataKey="tps"
                          name={t('usage.avgTps')}
                          stroke={tokens.status.info} strokeWidth={2} dot={false}
                        />
                      </LineChart>
                    </ResponsiveContainer>
                  </CardContent>
                </Card>
              )}

              {/* ── Error Rate / Hour ─────────────────────────── */}
              <Card>
                <CardHeader>
                  <CardTitle className="vds-text-base">{t('performance.errorRateTrend')}</CardTitle>
                </CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={180}>
                    <LineChart data={chartData}>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis
                        tick={AXIS_TICK} axisLine={false} tickLine={false} width={42}
                        domain={[0, 100]}
                        tickFormatter={(v) => `${v}%`}
                      />
                      <Tooltip
                        contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                        cursor={CURSOR_STROKE}
                        formatter={(v) => [`${Number(v)}%`, t('performance.errorRate')] as [string, string]}
                      />
                      <Line
                        type="monotone" dataKey="errorRate"
                        name={t('performance.errorRate')}
                        stroke={tokens.status.error} strokeWidth={2} dot={false}
                      />
                    </LineChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>
            </>
          )}
        </>
      )}
    </div>
  )
}
