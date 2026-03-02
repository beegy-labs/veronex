'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { performanceQuery } from '@/lib/queries'
import {
  LineChart, Line, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer,
  ReferenceLine, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL, CURSOR_STROKE,
  fmtMs, fmtMsAxis,
} from '@/lib/chart-theme'
import { Timer, TrendingUp, CheckCircle, AlertTriangle } from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'

const TIME_OPTIONS = [
  { label: '24h', hours: 24 },
  { label: '7d',  hours: 168 },
  { label: '30d', hours: 720 },
]

// fmtMs / fmtMsAxis imported from chart-theme
const ms = fmtMs

function pct(n: number) {
  return `${Math.round(n * 100)}%`
}

function fmt(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

function fmtHour(iso: string) {
  const d = new Date(iso)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}h`
}

export default function PerformancePage() {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)

  const { data, isLoading, error } = useQuery(performanceQuery(hours))

  const chartData = data?.hourly.map((h) => ({
    hour:      fmtHour(h.hour),
    latency:   Math.round(h.avg_latency_ms),
    total:     h.request_count,
    success:   h.success_count,
    errors:    Math.max(0, h.request_count - h.success_count),
    tokens:    h.total_tokens,
    errorRate: h.request_count > 0
      ? parseFloat(((h.request_count - h.success_count) / h.request_count * 100).toFixed(1))
      : 0,
  })) ?? []

  const hasData    = data && data.total_requests > 0
  const errorCount = data ? data.total_requests - Math.round(data.success_rate * data.total_requests) : 0
  const currentLabel = TIME_OPTIONS.find(o => o.hours === hours)?.label ?? `${hours}h`

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">
            {t('performance.title')}
          </h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('performance.description')}</p>
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          {TIME_OPTIONS.map((opt) => (
            <Button
              key={opt.hours}
              variant={hours === opt.hours ? 'default' : 'outline'}
              size="sm"
              onClick={() => setHours(opt.hours)}
            >
              {opt.label}
            </Button>
          ))}
        </div>
      </div>

      {/* ClickHouse unavailable */}
      {error && (
        <Card className="border-status-warning/30 bg-status-warning/10">
          <CardContent className="p-5">
            <p className="font-semibold text-status-warning-fg">{t('performance.analyticsUnavailable')}</p>
            <p className="text-sm mt-1 text-status-warning-fg/80">{t('performance.clickhouseDisabled')}</p>
          </CardContent>
        </Card>
      )}

      {isLoading && (
        <div className="grid grid-cols-3 sm:grid-cols-5 gap-4">
          {Array.from({ length: 5 }).map((_, i) => (
            <Card key={i}><CardContent className="p-6">
              <div className="h-3 w-24 rounded bg-muted animate-pulse mb-4" />
              <div className="h-8 w-16 rounded bg-muted animate-pulse" />
            </CardContent></Card>
          ))}
        </div>
      )}

      {!error && !isLoading && !hasData && (
        <Card>
          <CardContent className="p-10 text-center text-muted-foreground">
            <p className="font-medium">{t('performance.noData')}</p>
            <p className="text-sm mt-1">{t('performance.noDataHint')}</p>
          </CardContent>
        </Card>
      )}

      {!error && data && hasData && (
        <>
          {/* ── KPI cards (5) ───────────────────────────────── */}
          <div className="grid grid-cols-3 sm:grid-cols-5 gap-4">
            <StatsCard
              title={t('performance.p50')}
              value={ms(data.p50_latency_ms)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<Timer className="h-5 w-5" />}
            />
            <StatsCard
              title={t('performance.p95')}
              value={ms(data.p95_latency_ms)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<TrendingUp className="h-5 w-5" />}
            />
            <StatsCard
              title={t('performance.p99')}
              value={ms(data.p99_latency_ms)}
              subtitle={`avg ${ms(data.avg_latency_ms)}`}
              icon={<TrendingUp className="h-5 w-5" />}
            />
            <StatsCard
              title={t('performance.successRate')}
              value={pct(data.success_rate)}
              subtitle={`${fmt(data.total_requests)} ${t('overview.requests')}`}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title={t('performance.errors')}
              value={fmt(errorCount)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<AlertTriangle className={`h-5 w-5 ${errorCount > 0 ? 'text-[var(--theme-status-error)]' : ''}`} />}
            />
          </div>

          {/* ── Latency percentile boxes ─────────────────────── */}
          <Card>
            <CardHeader>
              <CardTitle className="text-base">{t('performance.latencyPercentiles')}</CardTitle>
              <p className="text-xs text-muted-foreground">{t('performance.aggregatedOver')}</p>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                {[
                  { label: t('performance.p50'),        value: ms(data.p50_latency_ms) },
                  { label: t('performance.p95'),        value: ms(data.p95_latency_ms) },
                  { label: t('performance.p99'),        value: ms(data.p99_latency_ms) },
                  { label: t('performance.avgLatency'), value: ms(data.avg_latency_ms) },
                ].map(({ label, value }) => (
                  <Card key={label} className="text-center">
                    <CardContent className="p-4">
                      <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-2">
                        {label}
                      </p>
                      <p className="text-2xl font-bold font-mono">{value}</p>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </CardContent>
          </Card>

          {chartData.length > 0 && (
            <>
              {/* ── Avg latency trend ─────────────────────────── */}
              <Card>
                <CardHeader>
                  <CardTitle className="text-base">{t('performance.avgLatencyHour')}</CardTitle>
                </CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={200}>
                    <LineChart data={chartData}>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={55} tickFormatter={fmtMsAxis} />
                      <Tooltip
                        contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                        cursor={CURSOR_STROKE}
                        formatter={(v) => [ms(Number(v)), t('performance.avgLatency')] as [string, string]}
                      />
                      <ReferenceLine
                        y={data.p95_latency_ms}
                        stroke="var(--theme-status-warning)"
                        strokeDasharray="4 4"
                        label={{ value: 'P95', position: 'right', fill: 'var(--theme-status-warning)', fontSize: 11 }}
                      />
                      <Line
                        type="monotone" dataKey="latency"
                        stroke="var(--theme-primary)" strokeWidth={2} dot={false}
                      />
                    </LineChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              {/* ── Throughput: total / success / errors ─────────── */}
              <Card>
                <CardHeader>
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-base">{t('performance.throughputHour')}</CardTitle>
                    {errorCount > 0 && (
                      <span className="flex items-center gap-1.5 text-xs text-[var(--theme-status-error)]">
                        <AlertTriangle className="h-3.5 w-3.5" />
                        {fmt(errorCount)} {t('performance.errors')}
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
                      <Bar dataKey="total"   name={t('overview.totalReqs')}   fill="var(--theme-primary)"        radius={[3, 3, 0, 0]} />
                      <Bar dataKey="success" name={t('overview.successReqs')} fill="var(--theme-status-success)" radius={[3, 3, 0, 0]} />
                      <Bar dataKey="errors"  name={t('performance.errors')}   fill="var(--theme-status-error)"   radius={[3, 3, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              {/* ── Error Rate / Hour ─────────────────────────────── */}
              <Card>
                <CardHeader>
                  <CardTitle className="text-base">{t('performance.errorRateTrend')}</CardTitle>
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
                        stroke="var(--theme-status-error)" strokeWidth={2} dot={false}
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
