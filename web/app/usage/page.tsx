'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  usageAggregateQuery, analyticsQuery, performanceQuery,
  usageBreakdownQuery, keysQuery,
} from '@/lib/queries'
import { fmtCompact, fmtCost } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import {
  Hash, Coins, CheckCircle, XCircle, AlertTriangle,
  Bot, Server, Key, DollarSign, Search,
} from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { useTranslation } from '@/i18n'
import { TIME_LABEL_MAP, TimeRangeSelector, type TimeRange } from '@/components/time-range-selector'
import { SectionLabel } from '@/components/section-label'

import { OverviewTab } from './components/overview-tab'
import { ByKeyTab } from './components/by-key-tab'
import { ProviderBreakdownSection } from './components/provider-breakdown'
import { ModelBreakdownTable } from './components/breakdown-tables'
import { ModelLatencyChart } from './components/model-latency-chart'

/* ─── page ────────────────────────────────────────────────── */
export default function UsagePage() {
  const { t } = useTranslation()
  const [range, setRange] = useState<TimeRange>({ hours: 24 })
  const hours = range.hours
  const [modelFilter, setModelFilter] = useState('')

  const { data: agg, isLoading: aggLoading, error: aggError } = useQuery(usageAggregateQuery(hours))
  const { data: analytics } = useQuery(analyticsQuery(hours))
  const { data: perf } = useQuery(performanceQuery(hours))
  const { data: breakdown } = useQuery(usageBreakdownQuery(hours))
  const { data: keysData } = useQuery(keysQuery())
  const keys = keysData?.keys

  const errorRate = agg && agg.request_count > 0
    ? calcPercentage(agg.error_count, agg.request_count) : 0

  const currentLabel = range.from
    ? `${range.from.slice(5, 16)} ~ ${(range.to ?? 'now').slice(5, 16)}`
    : TIME_LABEL_MAP.get(hours) ?? `${hours}h`

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('usage.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('usage.description')}</p>
        </div>
        <TimeRangeSelector value={range} onChange={setRange} />
      </div>

      {aggError && (
        <Card className="border-status-warning/30 bg-status-warning/10">
          <CardContent className="p-5">
            <p className="font-semibold text-status-warning-fg">{t('usage.analyticsUnavailable')}</p>
            <p className="text-sm mt-1 text-status-warning-fg/80">{t('usage.clickhouseDisabled')}</p>
          </CardContent>
        </Card>
      )}

      {/* ── KPI cards — always visible ────────────────── */}
      {aggLoading && (
        <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <Card key={i}><CardContent className="p-6">
              <div className="h-3 w-24 rounded bg-muted animate-pulse mb-4" />
              <div className="h-8 w-16 rounded bg-muted animate-pulse" />
            </CardContent></Card>
          ))}
        </div>
      )}

      {agg && !aggError && (
        <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
          <StatsCard title={t('usage.totalRequests')} value={fmtCompact(agg.request_count)}
            subtitle={`${t('common.last')} ${currentLabel}`} icon={<Hash className="h-5 w-5" />} />
          <StatsCard title={t('usage.totalTokens')} value={fmtCompact(agg.total_tokens)}
            subtitle={`${fmtCompact(agg.prompt_tokens)} prompt · ${fmtCompact(agg.completion_tokens)} compl`}
            icon={<Coins className="h-5 w-5" />} />
          <StatsCard title={t('usage.success')}
            value={agg.request_count > 0 ? `${calcPercentage(agg.success_count, agg.request_count)}%` : '—'}
            subtitle={`${fmtCompact(agg.success_count)} ${t('usage.completed')}`}
            icon={<CheckCircle className="h-5 w-5" />} />
          <StatsCard title={t('usage.errors')} value={fmtCompact(agg.error_count)}
            subtitle={`${fmtCompact(agg.cancelled_count)} ${t('usage.cancelled')}`}
            icon={errorRate >= 10
              ? <AlertTriangle className="h-5 w-5 text-status-error" />
              : <XCircle className="h-5 w-5" />} />
        </div>
      )}

      {/* Total cost badge */}
      {breakdown && breakdown.total_cost_usd > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/30 px-4 py-2 w-fit">
          <DollarSign className="h-4 w-4 text-muted-foreground" />
          <div>
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground font-bold">{t('usage.totalCost')}</p>
            <p className="text-lg font-bold tabular-nums font-mono">{fmtCost(breakdown.total_cost_usd)}</p>
          </div>
        </div>
      )}

      {agg && agg.request_count === 0 && !aggError && (
        <Card>
          <CardContent className="p-10 text-center text-muted-foreground">
            <p className="font-medium">{t('usage.noData')}</p>
            <p className="text-sm mt-1">{t('usage.noDataHint')}</p>
          </CardContent>
        </Card>
      )}

      {/* ── Tabs ──────────────────────────────────────── */}
      <Tabs defaultValue="overview">
        <TabsList>
          <TabsTrigger value="overview">{t('usage.overview')}</TabsTrigger>
          <TabsTrigger value="by-key">
            <Key className="h-3.5 w-3.5 mr-1.5" />
            {t('usage.byKey')}
          </TabsTrigger>
          <TabsTrigger value="by-model">
            <Bot className="h-3.5 w-3.5 mr-1.5" />
            {t('usage.byModel')}
          </TabsTrigger>
          <TabsTrigger value="by-provider">
            <Server className="h-3.5 w-3.5 mr-1.5" />
            {t('usage.byProvider')}
          </TabsTrigger>
        </TabsList>

        {/* ── Overview ──────────────────────────────── */}
        <TabsContent value="overview">
          <OverviewTab agg={agg} analytics={analytics} perf={perf} currentLabel={currentLabel} />
        </TabsContent>

        {/* ── By Key ──────────────────────────────────── */}
        <TabsContent value="by-key">
          <ByKeyTab breakdown={breakdown} keys={keys} hours={hours} />
        </TabsContent>

        {/* ── By Model ────────────────────────────────── */}
        <TabsContent value="by-model" className="space-y-6 mt-4">
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between flex-wrap gap-3">
                <div>
                  <CardTitle className="text-base flex items-center gap-2">
                    <Bot className="h-4 w-4 text-primary" />
                    {t('usage.byModel')}
                  </CardTitle>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('usage.modelCallRatio')}</p>
                </div>
                <div className="relative">
                  <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground" />
                  <Input
                    placeholder={t('usage.searchModels')}
                    value={modelFilter}
                    onChange={(e) => setModelFilter(e.target.value)}
                    className="pl-8 h-8 w-52 text-sm"
                  />
                </div>
              </div>
            </CardHeader>
            <CardContent>
              {!breakdown && (
                <div className="flex h-32 items-center justify-center text-muted-foreground text-sm">{t('common.loading')}</div>
              )}
              {breakdown && (
                <ModelBreakdownTable data={breakdown.by_model} filter={modelFilter} />
              )}
            </CardContent>
          </Card>

          {breakdown && breakdown.by_model.length > 0 && (
            <ModelLatencyChart data={breakdown.by_model} />
          )}
        </TabsContent>

        {/* ── By Provider ─────────────────────────────── */}
        <TabsContent value="by-provider" className="space-y-4 mt-4">
          <div>
            <SectionLabel className="mb-4">
              {t('usage.byProvider')}
            </SectionLabel>
            {!breakdown && (
              <div className="flex h-32 items-center justify-center text-muted-foreground text-sm">{t('common.loading')}</div>
            )}
            {breakdown && <ProviderBreakdownSection data={breakdown} />}
          </div>
        </TabsContent>
      </Tabs>
    </div>
  )
}
