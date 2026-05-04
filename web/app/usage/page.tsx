'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTimeRange } from '@/components/time-range-context'
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
import { usePageGuard } from '@/hooks/use-page-guard'
import { TIME_LABEL_MAP, TimeRangeSelector } from '@/components/time-range-selector'
import { SectionLabel } from '@/components/section-label'

import { OverviewTab } from './components/overview-tab'
import { ByKeyTab } from './components/by-key-tab'
import { ProviderBreakdownSection } from './components/provider-breakdown'
import { ModelBreakdownTable } from './components/breakdown-tables'
import { ModelLatencyChart } from './components/model-latency-chart'

/* ─── page ────────────────────────────────────────────────── */
export default function UsagePage() {
  usePageGuard('dashboard_view')
  const { t } = useTranslation()
  const { range, setRange } = useTimeRange()
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
    <div className="vds-space-y-6">
      {/* Header */}
      <div className="vds-flex vds-items-center vds-justify-between vds-flex-wrap vds-gap-4">
        <div>
          <h1 className="vds-text-2xl vds-font-700 vds-tracking-tight">{t('usage.title')}</h1>
          <p className="vds-text-dim vds-mt-1 vds-text-sm">{t('usage.description')}</p>
        </div>
        <TimeRangeSelector value={range} onChange={setRange} />
      </div>

      {aggError && (
        <Card className="vds-border-warning/30 vds-bg-warning/10">
          <CardContent className="vds-p-5">
            <p className="vds-font-600 vds-text-warning">{t('usage.analyticsUnavailable')}</p>
            <p className="vds-text-sm vds-mt-1 vds-text-warning/80">{t('usage.clickhouseDisabled')}</p>
          </CardContent>
        </Card>
      )}

      {/* ── KPI cards — always visible ────────────────── */}
      {aggLoading && (
        <div className="vds-grid vds-grid-cols-2 vds-xl:grid-cols-4 vds-gap-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <Card key={i}><CardContent className="vds-p-6">
              <div className="vds-h-3 vds-w-24 vds-rounded vds-bg-muted vds-animate-pulse vds-mb-4" />
              <div className="vds-h-8 vds-w-16 vds-rounded vds-bg-muted vds-animate-pulse" />
            </CardContent></Card>
          ))}
        </div>
      )}

      {agg && !aggError && (
        <div className="vds-grid vds-grid-cols-2 vds-xl:grid-cols-4 vds-gap-4">
          <StatsCard title={t('usage.totalRequests')} value={fmtCompact(agg.request_count)}
            subtitle={`${t('common.last')} ${currentLabel}`} icon={<Hash className="vds-h-5 vds-w-5" />} />
          <StatsCard title={t('usage.totalTokens')} value={fmtCompact(agg.total_tokens)}
            subtitle={`${fmtCompact(agg.prompt_tokens)} prompt · ${fmtCompact(agg.completion_tokens)} compl`}
            icon={<Coins className="vds-h-5 vds-w-5" />} />
          <StatsCard title={t('usage.success')}
            value={agg.request_count > 0 ? `${calcPercentage(agg.success_count, agg.request_count)}%` : '—'}
            subtitle={`${fmtCompact(agg.success_count)} ${t('usage.completed')}`}
            icon={<CheckCircle className="vds-h-5 vds-w-5" />} />
          <StatsCard title={t('usage.errors')} value={fmtCompact(agg.error_count)}
            subtitle={`${fmtCompact(agg.cancelled_count)} ${t('usage.cancelled')}`}
            icon={errorRate >= 10
              ? <AlertTriangle className="vds-h-5 vds-w-5 vds-text-error" />
              : <XCircle className="vds-h-5 vds-w-5" />} />
        </div>
      )}

      {/* Total cost badge */}
      {breakdown && breakdown.total_cost_usd > 0 && (
        <div className="vds-flex vds-items-center vds-gap-2 vds-rounded-lg vds-border-1 vds-border-subtle vds-bg-muted/30 vds-px-4 vds-py-2 vds-w-fit">
          <DollarSign className="vds-h-4 vds-w-4 vds-text-dim" />
          <div>
            <p className="vds-text-[10px] vds-uppercase vds-tracking-widest vds-text-dim vds-font-700">{t('usage.totalCost')}</p>
            <p className="vds-text-lg vds-font-700 vds-tabular-nums vds-font-mono">{fmtCost(breakdown.total_cost_usd)}</p>
          </div>
        </div>
      )}

      {agg && agg.request_count === 0 && !aggError && (
        <Card>
          <CardContent className="vds-p-10 vds-text-center vds-text-dim">
            <p className="vds-font-500">{t('usage.noData')}</p>
            <p className="vds-text-sm vds-mt-1">{t('usage.noDataHint')}</p>
          </CardContent>
        </Card>
      )}

      {/* ── Tabs ──────────────────────────────────────── */}
      <Tabs defaultValue="overview">
        <TabsList>
          <TabsTrigger value="overview">{t('usage.overview')}</TabsTrigger>
          <TabsTrigger value="by-key">
            <Key className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
            {t('usage.byKey')}
          </TabsTrigger>
          <TabsTrigger value="by-model">
            <Bot className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
            {t('usage.byModel')}
          </TabsTrigger>
          <TabsTrigger value="by-provider">
            <Server className="vds-h-3.5 vds-w-3.5 vds-mr-1.5" />
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
        <TabsContent value="by-model" className="vds-space-y-6 vds-mt-4">
          <Card>
            <CardHeader>
              <div className="vds-flex vds-items-center vds-justify-between vds-flex-wrap vds-gap-3">
                <div>
                  <CardTitle className="vds-text-base vds-flex vds-items-center vds-gap-2">
                    <Bot className="vds-h-4 vds-w-4 vds-text-primary" />
                    {t('usage.byModel')}
                  </CardTitle>
                  <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('usage.modelCallRatio')}</p>
                </div>
                <div className="vds-relative">
                  <Search className="vds-absolute vds-left-2.5 vds-top-2.5 vds-h-3.5 vds-w-3.5 vds-text-dim" />
                  <Input
                    placeholder={t('usage.searchModels')}
                    value={modelFilter}
                    onChange={(e) => setModelFilter(e.target.value)}
                    className="vds-pl-8 vds-h-8 vds-w-52 vds-text-sm"
                  />
                </div>
              </div>
            </CardHeader>
            <CardContent>
              {!breakdown && (
                <div className="vds-flex vds-h-32 vds-items-center vds-justify-center vds-text-dim vds-text-sm">{t('common.loading')}</div>
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
        <TabsContent value="by-provider" className="vds-space-y-4 vds-mt-4">
          <div>
            <SectionLabel className="vds-mb-4">
              {t('usage.byProvider')}
            </SectionLabel>
            {!breakdown && (
              <div className="vds-flex vds-h-32 vds-items-center vds-justify-center vds-text-dim vds-text-sm">{t('common.loading')}</div>
            )}
            {breakdown && <ProviderBreakdownSection data={breakdown} />}
          </div>
        </TabsContent>
      </Tabs>
    </div>
  )
}
