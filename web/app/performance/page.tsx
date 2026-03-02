'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { performanceQuery, usageBreakdownQuery, analyticsQuery } from '@/lib/queries'
import {
  LineChart, Line, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer,
  ReferenceLine, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL, CURSOR_STROKE,
  fmtMs, fmtMsAxis, fmtCompact,
} from '@/lib/chart-theme'
import { Timer, TrendingUp, CheckCircle, AlertTriangle, Zap, Key, Bot } from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { TIME_OPTIONS, TimeRangeSelector } from '@/components/time-range-selector'
import { fmtHourLabel } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'

const ms = fmtMs

const BACKEND_BADGE: Record<string, string> = {
  ollama: 'bg-primary/10 text-primary border-primary/30',
  gemini: 'bg-status-info/10 text-status-info-fg border-status-info/30',
}

function pct(n: number) {
  return `${Math.round(n * 100)}%`
}

/* ─── Model latency comparison ────────────────────────────── */
function ModelLatencySection({
  models,
}: {
  models: { model_name: string; provider_type: string; request_count: number; avg_latency_ms: number; success_rate?: number }[]
}) {
  const { t } = useTranslation()
  if (models.length === 0) return null

  const chartData = models
    .filter((m) => m.avg_latency_ms > 0)
    .sort((a, b) => a.avg_latency_ms - b.avg_latency_ms)
    .slice(0, 10)
    .map((m) => ({
      name: m.model_name.length > 24 ? m.model_name.slice(0, 23) + '…' : m.model_name,
      latency: Math.round(m.avg_latency_ms),
    }))

  return (
    <div className="grid grid-cols-1 xl:grid-cols-5 gap-4">
      {/* Table */}
      <Card className="xl:col-span-3">
        <CardHeader>
          <CardTitle className="text-base flex items-center gap-2">
            <Bot className="h-4 w-4 text-primary" />
            {t('performance.byModel')}
          </CardTitle>
          <p className="text-xs text-muted-foreground">{t('performance.modelLatency')}</p>
        </CardHeader>
        <CardContent>
          <DataTable minWidth="500px">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Model</TableHead>
                <TableHead className="w-28">{t('usage.providerCol')}</TableHead>
                <TableHead className="text-right w-24">Requests</TableHead>
                <TableHead className="text-right w-32">Avg Latency</TableHead>
                <TableHead className="text-right w-24">Success</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {models.map((m, i) => (
                <TableRow key={`${m.model_name}-${m.provider_type}-${i}`}>
                  <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
                  <TableCell>
                    <Badge variant="outline" className={`text-xs ${BACKEND_BADGE[m.provider_type] ?? ''}`}>
                      {m.provider_type}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{fmtCompact(m.request_count)}</TableCell>
                  <TableCell className="text-right tabular-nums font-semibold">
                    {m.avg_latency_ms > 0 ? ms(m.avg_latency_ms) : '—'}
                  </TableCell>
                  <TableCell className="text-right">
                    {m.success_rate != null ? (
                      <span className={`text-sm font-semibold tabular-nums ${
                        (m.success_rate * 100) >= 90 ? 'text-status-success-fg'
                          : (m.success_rate * 100) >= 70 ? 'text-status-warning-fg'
                          : 'text-status-error-fg'
                      }`}>
                        {pct(m.success_rate)}
                      </span>
                    ) : '—'}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </DataTable>
        </CardContent>
      </Card>

      {/* Bar chart */}
      {chartData.length > 0 && (
        <Card className="xl:col-span-2">
          <CardHeader>
            <CardTitle className="text-base">{t('performance.modelLatency')}</CardTitle>
            <p className="text-xs text-muted-foreground">avg ms (top 10)</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={Math.max(160, chartData.length * 34)}>
              <BarChart data={chartData} layout="vertical" margin={{ left: 4, right: 24 }}>
                <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={(v) => `${v}ms`} />
                <YAxis
                  type="category" dataKey="name" width={130}
                  tick={{ ...AXIS_TICK, fontSize: 10 }}
                  axisLine={false} tickLine={false}
                />
                <Tooltip
                  contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                  cursor={CURSOR_FILL}
                  formatter={(v) => [`${v}ms`, t('performance.avgLatency')] as [string, string]}
                />
                <Bar dataKey="latency" name={t('performance.avgLatency')} fill="var(--theme-status-info)" radius={[0, 4, 4, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

/* ─── Per-key performance ─────────────────────────────────── */
function KeyPerformanceSection({
  keys,
}: {
  keys: { key_id: string; key_name: string; key_prefix: string; request_count: number; success_rate: number; prompt_tokens: number; completion_tokens: number; estimated_cost_usd: number | null }[]
}) {
  const { t } = useTranslation()
  if (keys.length === 0) return null

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base flex items-center gap-2">
          <Key className="h-4 w-4 text-primary" />
          {t('performance.byKey')}
        </CardTitle>
        <p className="text-xs text-muted-foreground">{t('performance.keyPerformance')}</p>
      </CardHeader>
      <CardContent>
        <DataTable minWidth="640px">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>{t('performance.keyCol')}</TableHead>
              <TableHead className="text-right w-24">Requests</TableHead>
              <TableHead className="text-right w-28">Success</TableHead>
              <TableHead className="text-right w-28">Tokens</TableHead>
              <TableHead className="text-right w-28">Est. Cost</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {keys.map((k) => {
              const totalTok = k.prompt_tokens + k.completion_tokens
              return (
                <TableRow key={k.key_id}>
                  <TableCell>
                    <p className="font-semibold text-sm">{k.key_name}</p>
                    <p className="text-xs text-muted-foreground font-mono">{k.key_prefix}…</p>
                  </TableCell>
                  <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(k.request_count)}</TableCell>
                  <TableCell className="text-right">
                    <span className={`text-sm font-semibold tabular-nums ${
                      k.success_rate >= 90 ? 'text-status-success-fg'
                        : k.success_rate >= 70 ? 'text-status-warning-fg'
                        : 'text-status-error-fg'
                    }`}>
                      {k.success_rate}%
                    </span>
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                    {fmtCompact(totalTok)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-sm font-mono">
                    {k.estimated_cost_usd == null
                      ? <span className="text-muted-foreground">—</span>
                      : k.estimated_cost_usd === 0
                        ? <span className="text-muted-foreground">Free</span>
                        : <span>${k.estimated_cost_usd.toFixed(4)}</span>}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </DataTable>
      </CardContent>
    </Card>
  )
}

/* ─── page ────────────────────────────────────────────────── */
export default function PerformancePage() {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [hours, setHours] = useState(24)

  const { data, isLoading, error } = useQuery(performanceQuery(hours))
  const { data: breakdown } = useQuery(usageBreakdownQuery(hours))
  const { data: analytics } = useQuery(analyticsQuery(hours))

  const chartData = data?.hourly.map((h) => ({
    hour:      fmtHourLabel(h.hour, tz),
    latency:   Math.round(h.avg_latency_ms),
    total:     h.request_count,
    success:   h.success_count,
    errors:    Math.max(0, h.request_count - h.success_count),
    tokens:    h.total_tokens,
    errorRate: h.request_count > 0
      ? parseFloat(((h.request_count - h.success_count) / h.request_count * 100).toFixed(1))
      : 0,
    tps: h.request_count > 0 && h.total_tokens > 0
      ? parseFloat((h.total_tokens / (h.avg_latency_ms / 1000 * h.request_count)).toFixed(1))
      : 0,
  })) ?? []

  const hasData    = data && data.total_requests > 0
  const errorCount = data ? data.total_requests - Math.round(data.success_rate * data.total_requests) : 0
  const currentLabel = TIME_OPTIONS.find(o => o.hours === hours)?.label ?? `${hours}h`

  // Merge analytics model stats (has success_rate) with breakdown model data (has avg_latency_ms)
  const modelPerfData = (() => {
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
  })()

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('performance.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('performance.description')}</p>
        </div>
        <TimeRangeSelector value={hours} onChange={setHours} />
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
              subtitle={`${fmtCompact(data.total_requests)} ${t('overview.requests')}`}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title={t('performance.errors')}
              value={fmtCompact(errorCount)}
              subtitle={`${t('common.last')} ${currentLabel}`}
              icon={<AlertTriangle className={`h-5 w-5 ${errorCount > 0 ? 'text-[var(--theme-status-error)]' : ''}`} />}
            />
          </div>

          {/* ── Analytics TPS card (if available) ──────── */}
          {analytics && (
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
              <StatsCard
                title={t('usage.avgTps')}
                value={analytics.avg_tps > 0 ? analytics.avg_tps.toFixed(2) : '—'}
                subtitle={t('usage.avgTpsDesc')}
                icon={<Zap className="h-5 w-5" />}
              />
              <StatsCard
                title={t('usage.avgPromptTokens')}
                value={analytics.avg_prompt_tokens > 0 ? fmtCompact(analytics.avg_prompt_tokens) : '—'}
                subtitle={t('usage.tokensPerReq')}
                icon={<Timer className="h-5 w-5" />}
              />
              <StatsCard
                title={t('usage.avgCompletionTokens')}
                value={analytics.avg_completion_tokens > 0 ? fmtCompact(analytics.avg_completion_tokens) : '—'}
                subtitle={t('usage.tokensPerReq')}
                icon={<Timer className="h-5 w-5" />}
              />
              <StatsCard
                title={t('performance.totalRequests')}
                value={fmtCompact(data.total_requests)}
                subtitle={`${t('common.last')} ${currentLabel}`}
                icon={<CheckCircle className="h-5 w-5" />}
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
                  <CardTitle className="text-base">{t('performance.avgLatencyHour')}</CardTitle>
                  <p className="text-xs text-muted-foreground">
                    P95 reference line: {ms(data.p95_latency_ms)}
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

              {/* ── Throughput: total / success / errors ─────── */}
              <Card>
                <CardHeader>
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-base">{t('performance.throughputHour')}</CardTitle>
                    {errorCount > 0 && (
                      <span className="flex items-center gap-1.5 text-xs text-[var(--theme-status-error)]">
                        <AlertTriangle className="h-3.5 w-3.5" />
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
                      <Bar dataKey="total"   name={t('overview.totalReqs')}   fill="var(--theme-primary)"        radius={[3, 3, 0, 0]} />
                      <Bar dataKey="success" name={t('overview.successReqs')} fill="var(--theme-status-success)" radius={[3, 3, 0, 0]} />
                      <Bar dataKey="errors"  name={t('performance.errors')}   fill="var(--theme-status-error)"   radius={[3, 3, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              {/* ── TPS trend ──────────────────────────────────── */}
              {chartData.some((d) => d.tps > 0) && (
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">{t('performance.tpsHour')}</CardTitle>
                    {analytics && analytics.avg_tps > 0 && (
                      <p className="text-xs text-muted-foreground">
                        {t('usage.avgTps')}: <span className="font-semibold text-foreground">{analytics.avg_tps.toFixed(2)}</span> tok/s
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
                          formatter={(v) => [`${Number(v).toFixed(1)} tok/s`, t('usage.avgTps')] as [string, string]}
                        />
                        <Line
                          type="monotone" dataKey="tps"
                          name={t('usage.avgTps')}
                          stroke="var(--theme-status-info)" strokeWidth={2} dot={false}
                        />
                      </LineChart>
                    </ResponsiveContainer>
                  </CardContent>
                </Card>
              )}

              {/* ── Error Rate / Hour ─────────────────────────── */}
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
