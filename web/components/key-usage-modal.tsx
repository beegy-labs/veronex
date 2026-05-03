'use client'

import { useState, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { keyUsageQuery, keyModelBreakdownQuery } from '@/lib/queries'
import type { ApiKey } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL, fmtCompact, fmtMs, fmtPct1,
} from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { Hash, Coins, CheckCircle, XCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import StatsCard from '@/components/stats-card'
import { useTranslation } from '@/i18n'
import { TimeRangeSelector, type TimeRange } from '@/components/time-range-selector'
import { fmtHourLabel } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'
import { tokens } from '@/lib/design-tokens'
import { SectionLabel } from '@/components/section-label'

export function KeyUsageModal({
  apiKey,
  onClose,
}: {
  apiKey: ApiKey
  onClose: () => void
}) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [range, setRange] = useState<TimeRange>({ hours: 24 })
  const hours = range.hours

  const { data: hourly, isLoading } = useQuery(keyUsageQuery(apiKey.id, hours))
  const { data: models } = useQuery(keyModelBreakdownQuery(apiKey.id, hours))

  const chartData = useMemo(() =>
    (hourly ?? []).map((h) => ({
      hour:     fmtHourLabel(h.hour, tz),
      tokens:   h.total_tokens,
      prompt:   h.prompt_tokens,
      compl:    h.completion_tokens,
      requests: h.request_count,
      success:  h.success_count,
      errors:   h.error_count,
    })),
    [hourly, tz],
  )

  // Aggregate KPIs from hourly data
  const { totalRequests, totalTokens, totalSuccess, totalErrors, successRate } = useMemo(() => {
    const totalRequests = chartData.reduce((s, h) => s + h.requests, 0)
    const totalTokens   = chartData.reduce((s, h) => s + h.tokens, 0)
    const totalSuccess  = chartData.reduce((s, h) => s + h.success, 0)
    const totalErrors   = chartData.reduce((s, h) => s + h.errors, 0)
    const successRate   = totalRequests > 0
      ? calcPercentage(totalSuccess, totalRequests) : 0
    return { totalRequests, totalTokens, totalSuccess, totalErrors, successRate }
  }, [chartData])

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="vds-max-w-[95vw] vds-sm:max-w-3xl vds-max-h-[90vh] vds-overflow-y-auto">
        <DialogHeader>
          <div className="vds-flex vds-items-center vds-justify-between vds-gap-3 vds-flex-wrap">
            <div>
              <DialogTitle className="vds-text-lg">
                {t('keys.usageTitle', { name: apiKey.name })}
              </DialogTitle>
              <div className="vds-flex vds-items-center vds-gap-2 vds-mt-1">
                <code className="vds-text-xs vds-font-mono vds-text-dim">{apiKey.key_prefix}…</code>
                <Badge
                  variant="outline"
                  className={
                    apiKey.tier === 'free'
                      ? 'vds-text-dim vds-border-subtle vds-text-[10px] vds-whitespace-nowrap'
                      : 'vds-bg-info/10 vds-text-info vds-border-info/30 vds-text-[10px] vds-whitespace-nowrap'
                  }
                >
                  {apiKey.tier === 'free' ? t('keys.tierFree') : t('keys.tierPaid')}
                </Badge>
              </div>
            </div>
            <div className="vds-flex vds-items-center vds-gap-1">
              <TimeRangeSelector value={range} onChange={setRange} />
            </div>
          </div>
        </DialogHeader>

        {isLoading && (
          <div className="vds-flex vds-h-48 vds-items-center vds-justify-center vds-text-dim vds-text-sm">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && (
          <div className="vds-space-y-6 vds-mt-2">
            {/* KPI row */}
            <div className="vds-grid vds-grid-cols-2 vds-sm:grid-cols-4 vds-gap-3">
              <StatsCard
                title={t('usage.totalRequests')}
                value={fmtCompact(totalRequests)}
                icon={<Hash className="vds-h-4 vds-w-4" />}
              />
              <StatsCard
                title={t('usage.totalTokens')}
                value={fmtCompact(totalTokens)}
                icon={<Coins className="vds-h-4 vds-w-4" />}
              />
              <StatsCard
                title={t('usage.success')}
                value={totalRequests > 0 ? `${successRate}%` : '—'}
                icon={<CheckCircle className="vds-h-4 vds-w-4" />}
              />
              <StatsCard
                title={t('usage.errors')}
                value={fmtCompact(totalErrors)}
                icon={<XCircle className="vds-h-4 vds-w-4" />}
              />
            </div>

            {/* Model breakdown table */}
            {models && models.length > 0 && (
              <div>
                <SectionLabel>
                  {t('keys.modelBreakdown')}
                </SectionLabel>
                <DataTable minWidth="480px">
                  <TableHeader>
                    <TableRow>
                      <TableHead className="vds-whitespace-nowrap">{t('jobs.model')}</TableHead>
                      <TableHead className="vds-whitespace-nowrap">{t('usage.provider')}</TableHead>
                      <TableHead className="vds-text-right vds-whitespace-nowrap">{t('usage.requests')}</TableHead>
                      <TableHead className="vds-text-right vds-whitespace-nowrap">{t('usage.share')}</TableHead>
                      <TableHead className="vds-text-right vds-whitespace-nowrap">{t('usage.totalTokens')}</TableHead>
                      <TableHead className="vds-text-right vds-whitespace-nowrap">{t('usage.avgLatency')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {models.map((m) => (
                      <TableRow key={`${m.model_name}-${m.provider_type}`}>
                        <TableCell className="vds-font-mono vds-text-xs">{m.model_name}</TableCell>
                        <TableCell>
                          <Badge variant="outline" className="vds-text-[10px] vds-capitalize vds-whitespace-nowrap">{m.provider_type}</Badge>
                        </TableCell>
                        <TableCell className="vds-text-right vds-tabular-nums">{fmtCompact(m.request_count)}</TableCell>
                        <TableCell className="vds-text-right vds-tabular-nums vds-text-dim">
                          {fmtPct1(m.call_pct)}
                        </TableCell>
                        <TableCell className="vds-text-right vds-tabular-nums">
                          {fmtCompact(m.prompt_tokens + m.completion_tokens)}
                        </TableCell>
                        <TableCell className="vds-text-right vds-tabular-nums vds-text-dim">
                          {m.avg_latency_ms > 0 ? fmtMs(m.avg_latency_ms) : '—'}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </DataTable>
              </div>
            )}

            {chartData.length === 0 ? (
              <div className="vds-flex vds-h-32 vds-items-center vds-justify-center vds-text-dim vds-text-sm vds-rounded-lg vds-border-1 vds-border-dashed">
                {t('usage.noKeyData')}
              </div>
            ) : (
              <>
                {/* Token chart */}
                <div>
                  <SectionLabel>
                    {t('usage.tokensPerHour')}
                  </SectionLabel>
                  <ResponsiveContainer width="100%" height={180}>
                    <AreaChart data={chartData}>
                      <defs>
                        <linearGradient id="ku-gradPrompt" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%"  stopColor={tokens.brand.primary} stopOpacity={0.35} />
                          <stop offset="95%" stopColor={tokens.brand.primary} stopOpacity={0} />
                        </linearGradient>
                        <linearGradient id="ku-gradCompl" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%"  stopColor={tokens.status.info} stopOpacity={0.3} />
                          <stop offset="95%" stopColor={tokens.status.info} stopOpacity={0} />
                        </linearGradient>
                      </defs>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={42} tickFormatter={fmtCompact} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v) => fmtCompact(Number(v))} />
                      <Legend wrapperStyle={LEGEND_STYLE} />
                      <Area type="monotone" dataKey="prompt" name={t('usage.prompt')}     stroke={tokens.brand.primary}  fill="url(#ku-gradPrompt)" strokeWidth={2} dot={false} />
                      <Area type="monotone" dataKey="compl"  name={t('usage.completion')} stroke={tokens.status.info}    fill="url(#ku-gradCompl)"  strokeWidth={2} dot={false} />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>

                {/* Request chart */}
                <div>
                  <SectionLabel>
                    {t('usage.requestsPerHour')}
                  </SectionLabel>
                  <ResponsiveContainer width="100%" height={160}>
                    <BarChart data={chartData} barGap={2}>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={35} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} />
                      <Legend wrapperStyle={LEGEND_STYLE} />
                      <Bar dataKey="requests" name={t('usage.requests')} fill={tokens.brand.primary}    radius={[3, 3, 0, 0]} />
                      <Bar dataKey="success"  name={t('usage.success')}  fill={tokens.status.success} radius={[3, 3, 0, 0]} />
                      <Bar dataKey="errors"   name={t('usage.errors')}   fill={tokens.status.error}   radius={[3, 3, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
