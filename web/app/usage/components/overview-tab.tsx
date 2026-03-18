'use client'

import { useMemo } from 'react'
import type { UsageAggregate, AnalyticsStats, PerformanceStats } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtCompact, fmtTps,
} from '@/lib/chart-theme'
import { Zap, MessageSquare, Bot } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useTranslation } from '@/i18n'
import { fmtHourLabel } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'
import { tokens } from '@/lib/design-tokens'

import { TokenDonut } from './token-donut'
import { FinishReasonsCard } from './finish-reasons-card'

interface OverviewTabProps {
  agg: UsageAggregate | undefined
  analytics: AnalyticsStats | undefined
  perf: PerformanceStats | undefined
  currentLabel: string
}

export function OverviewTab({ agg, analytics, perf, currentLabel }: OverviewTabProps) {
  const { t } = useTranslation()
  const { tz } = useTimezone()

  const globalTrendData = useMemo(() =>
    (perf?.hourly ?? []).map((h) => ({
      hour:     fmtHourLabel(h.hour, tz),
      requests: h.request_count,
      tokens:   h.total_tokens,
    })),
    [perf?.hourly, tz],
  )

  const modelDistData = useMemo(() =>
    (analytics?.models ?? [])
      .slice()
      .sort((a, b) => b.request_count - a.request_count)
      .slice(0, 8)
      .map((m) => ({ name: m.model_name, requests: m.request_count })),
    [analytics?.models],
  )

  return (
    <div className="space-y-6 mt-4">
      {/* Global trend */}
      {globalTrendData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('performance.throughputHour')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('common.last')} {currentLabel}</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={globalTrendData}>
                <defs>
                  <linearGradient id="gradReqs" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%"  stopColor={tokens.brand.primary} stopOpacity={0.25} />
                    <stop offset="95%" stopColor={tokens.brand.primary} stopOpacity={0} />
                  </linearGradient>
                  <linearGradient id="gradToks" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%"  stopColor={tokens.status.info} stopOpacity={0.2} />
                    <stop offset="95%" stopColor={tokens.status.info} stopOpacity={0} />
                  </linearGradient>
                </defs>
                <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                <YAxis yAxisId="left" tick={AXIS_TICK} axisLine={false} tickLine={false} width={40} tickFormatter={fmtCompact} />
                <YAxis yAxisId="right" orientation="right" tick={AXIS_TICK} axisLine={false} tickLine={false} width={50} tickFormatter={fmtCompact} />
                <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v) => fmtCompact(Number(v))} />
                <Legend wrapperStyle={LEGEND_STYLE} />
                <Area yAxisId="left" type="monotone" dataKey="requests" name={t('usage.requests')}
                  stroke={tokens.brand.primary} fill="url(#gradReqs)" strokeWidth={2} dot={false} />
                <Area yAxisId="right" type="monotone" dataKey="tokens" name={t('usage.totalTokens')}
                  stroke={tokens.status.info} fill="url(#gradToks)" strokeWidth={2} dot={false} />
              </AreaChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}

      {/* Token donut + analytics KPIs */}
      <div className="grid grid-cols-1 xl:grid-cols-2 gap-6">
        {agg && agg.total_tokens > 0 && (
          <TokenDonut prompt={agg.prompt_tokens} completion={agg.completion_tokens} />
        )}
        {analytics && (
          <Card className="h-full">
            <CardHeader>
              <CardTitle className="text-base">{t('usage.analyticsTitle')}</CardTitle>
              <p className="text-xs text-muted-foreground">{t('usage.analyticsDesc')}</p>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="rounded-lg border border-border p-3 text-center">
                  <Zap className="h-4 w-4 mx-auto mb-1.5 text-muted-foreground" />
                  <p className="text-xl font-bold tabular-nums">
                    {fmtTps(analytics.avg_tps)}
                  </p>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-widest mt-0.5">{t('usage.avgTps')}</p>
                </div>
                <div className="rounded-lg border border-border p-3 text-center">
                  <MessageSquare className="h-4 w-4 mx-auto mb-1.5 text-muted-foreground" />
                  <p className="text-xl font-bold tabular-nums">
                    {analytics.avg_prompt_tokens > 0 ? fmtCompact(analytics.avg_prompt_tokens) : '—'}
                  </p>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-widest mt-0.5">{t('usage.avgPromptTokens')}</p>
                </div>
                <div className="rounded-lg border border-border p-3 text-center">
                  <Bot className="h-4 w-4 mx-auto mb-1.5 text-muted-foreground" />
                  <p className="text-xl font-bold tabular-nums">
                    {analytics.avg_completion_tokens > 0 ? fmtCompact(analytics.avg_completion_tokens) : '—'}
                  </p>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-widest mt-0.5">{t('usage.avgCompletionTokens')}</p>
                </div>
              </div>
              <FinishReasonsCard data={analytics} />
            </CardContent>
          </Card>
        )}
      </div>

      {/* Model distribution bar */}
      {analytics && modelDistData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('usage.modelDistTitle')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('common.last')} {currentLabel}</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={Math.max(160, modelDistData.length * 36)}>
              <BarChart
                data={modelDistData}
                layout="vertical" margin={{ left: 8, right: 16 }}
              >
                <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={fmtCompact} />
                <YAxis
                  type="category" dataKey="name" width={150}
                  tick={{ ...AXIS_TICK, fontSize: 10 }}
                  axisLine={false} tickLine={false}
                  tickFormatter={(v: string) => v.length > 22 ? v.slice(0, 21) + '…' : v}
                />
                <Tooltip
                  contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                  cursor={CURSOR_FILL}
                  formatter={(v) => [fmtCompact(Number(v)), t('usage.reqCount')] as [string, string]}
                />
                <Bar dataKey="requests" name={t('usage.reqCount')} fill={tokens.brand.primary} radius={[0, 4, 4, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
