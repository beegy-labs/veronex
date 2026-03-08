'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { keyUsageQuery, keyModelBreakdownQuery } from '@/lib/queries'
import type { ApiKey, UsageBreakdown, ModelBreakdown } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtMs, fmtCompact,
} from '@/lib/chart-theme'
import { Key } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select, SelectContent, SelectItem,
  SelectTrigger, SelectValue,
} from '@/components/ui/select'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { KeyUsageModal } from '@/components/key-usage-modal'
import { fmtHourLabel } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'
import { PROVIDER_BADGE, PROVIDER_COLORS } from '@/lib/constants'
import { KeyBreakdownTable } from './breakdown-tables'

interface ByKeyTabProps {
  breakdown: UsageBreakdown | undefined
  keys: ApiKey[] | undefined
  hours: number
}

export function ByKeyTab({ breakdown, keys, hours }: ByKeyTabProps) {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [selectedKeyId, setSelectedKeyId] = useState<string | null>(null)
  const activeKeyId = selectedKeyId ?? keys?.[0]?.id ?? null
  const [usageModalKey, setUsageModalKey] = useState<ApiKey | null>(null)

  const { data: hourly, isLoading: hourlyLoading } = useQuery(keyUsageQuery(activeKeyId, hours))
  const { data: keyModels } = useQuery(keyModelBreakdownQuery(activeKeyId, hours))

  const chartData = hourly?.map((h) => ({
    hour:     fmtHourLabel(h.hour, tz),
    tokens:   h.total_tokens,
    prompt:   h.prompt_tokens,
    compl:    h.completion_tokens,
    requests: h.request_count,
    success:  h.success_count,
    errors:   h.error_count,
  })) ?? []

  return (
    <div className="space-y-6 mt-4">
      {!breakdown && (
        <div className="flex h-32 items-center justify-center text-muted-foreground text-sm">{t('common.loading')}</div>
      )}
      {breakdown && (
        <>
          {/* Key breakdown table */}
          <Card>
            <CardHeader>
              <CardTitle className="text-base flex items-center gap-2">
                <Key className="h-4 w-4 text-primary" />
                {t('usage.byKey')}
              </CardTitle>
              <p className="text-xs text-muted-foreground">{t('usage.breakdownDesc')}</p>
            </CardHeader>
            <CardContent>
              <KeyBreakdownTable
                data={breakdown}
                keys={keys}
                selectedKeyId={activeKeyId}
                onKeyClick={setUsageModalKey}
                onKeySelect={setSelectedKeyId}
              />
            </CardContent>
          </Card>

          {/* Per-key detail */}
          {keys && keys.length > 0 && (
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between flex-wrap gap-3">
                  <CardTitle className="text-base">{t('usage.keyDetail')}</CardTitle>
                  <Select value={activeKeyId ?? ''} onValueChange={setSelectedKeyId}>
                    <SelectTrigger className="w-60">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {keys.map((k) => (
                        <SelectItem key={k.id} value={k.id}>
                          {k.name} ({k.key_prefix}…)
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </CardHeader>
              <CardContent className="space-y-8">
                {hourlyLoading && (
                  <div className="flex h-48 items-center justify-center text-muted-foreground text-sm">
                    {t('common.loading')}
                  </div>
                )}
                {!hourlyLoading && chartData.length === 0 && (
                  <div className="flex h-48 items-center justify-center text-muted-foreground text-sm">
                    {t('usage.noKeyData')}
                  </div>
                )}
                {!hourlyLoading && chartData.length > 0 && (
                  <>
                    {/* Tokens per hour */}
                    <div>
                      <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
                        {t('usage.tokensPerHour')}
                      </p>
                      <ResponsiveContainer width="100%" height={200}>
                        <AreaChart data={chartData}>
                          <defs>
                            <linearGradient id="gradPrompt" x1="0" y1="0" x2="0" y2="1">
                              <stop offset="5%"  stopColor="var(--theme-primary)" stopOpacity={0.35} />
                              <stop offset="95%" stopColor="var(--theme-primary)" stopOpacity={0} />
                            </linearGradient>
                            <linearGradient id="gradCompl" x1="0" y1="0" x2="0" y2="1">
                              <stop offset="5%"  stopColor="var(--theme-status-info)" stopOpacity={0.3} />
                              <stop offset="95%" stopColor="var(--theme-status-info)" stopOpacity={0} />
                            </linearGradient>
                          </defs>
                          <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                          <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={45} tickFormatter={fmtCompact} />
                          <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v) => fmtCompact(Number(v))} />
                          <Legend wrapperStyle={LEGEND_STYLE} />
                          <Area type="monotone" dataKey="prompt" name={t('usage.prompt')}     stroke="var(--theme-primary)"      fill="url(#gradPrompt)" strokeWidth={2} dot={false} />
                          <Area type="monotone" dataKey="compl"  name={t('usage.completion')} stroke="var(--theme-status-info)"  fill="url(#gradCompl)"  strokeWidth={2} dot={false} />
                        </AreaChart>
                      </ResponsiveContainer>
                    </div>
                    {/* Requests per hour */}
                    <div>
                      <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
                        {t('usage.requestsPerHour')}
                      </p>
                      <ResponsiveContainer width="100%" height={180}>
                        <BarChart data={chartData} barGap={2}>
                          <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                          <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={35} />
                          <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} />
                          <Legend wrapperStyle={LEGEND_STYLE} />
                          <Bar dataKey="requests" name={t('usage.requests')} fill="var(--theme-primary)"         radius={[3, 3, 0, 0]} />
                          <Bar dataKey="success"  name={t('usage.success')}  fill="var(--theme-status-success)" radius={[3, 3, 0, 0]} />
                          <Bar dataKey="errors"   name={t('usage.errors')}   fill="var(--theme-status-error)"   radius={[3, 3, 0, 0]} />
                        </BarChart>
                      </ResponsiveContainer>
                    </div>
                  </>
                )}

                {/* Key model breakdown */}
                {keyModels && keyModels.length > 0 && (
                  <div>
                    <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
                      {t('usage.modelCallRatio')}
                    </p>
                    <DataTable minWidth="600px">
                      <TableHeader>
                        <TableRow className="hover:bg-transparent">
                          <TableHead>{t('usage.modelCol')}</TableHead>
                          <TableHead className="w-28">{t('usage.providerCol')}</TableHead>
                          <TableHead className="text-right w-24">{t('usage.reqCount')}</TableHead>
                          <TableHead className="w-36">{t('usage.share')}</TableHead>
                          <TableHead className="text-right w-32">{t('usage.avgLatency')}</TableHead>
                          <TableHead className="text-right w-28">{t('jobs.tokens')}</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {keyModels.map((m, i) => {
                          const totalTok = m.prompt_tokens + m.completion_tokens
                          const color = PROVIDER_COLORS[m.provider_type] ?? 'var(--theme-primary)'
                          return (
                            <TableRow key={`${m.model_name}-${i}`}>
                              <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
                              <TableCell>
                                <Badge variant="outline" className={`text-xs ${PROVIDER_BADGE[m.provider_type] ?? ''}`}>
                                  {m.provider_type}
                                </Badge>
                              </TableCell>
                              <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(m.request_count)}</TableCell>
                              <TableCell>
                                <div className="flex items-center gap-2">
                                  <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
                                    <div className="h-full rounded-full" style={{ width: `${Math.min(m.call_pct, 100)}%`, background: color }} />
                                  </div>
                                  <span className="text-xs tabular-nums w-10 text-right font-semibold" style={{ color }}>
                                    {m.call_pct.toFixed(1)}%
                                  </span>
                                </div>
                              </TableCell>
                              <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                                {m.avg_latency_ms > 0 ? fmtMs(m.avg_latency_ms) : '—'}
                              </TableCell>
                              <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                                {fmtCompact(totalTok)}
                              </TableCell>
                            </TableRow>
                          )
                        })}
                      </TableBody>
                    </DataTable>
                  </div>
                )}
              </CardContent>
            </Card>
          )}

          {(!keys || keys.length === 0) && (
            <Card>
              <CardContent className="p-6 text-center text-muted-foreground text-sm">
                {t('usage.noKeysMsg')}
              </CardContent>
            </Card>
          )}
        </>
      )}

      {usageModalKey && (
        <KeyUsageModal apiKey={usageModalKey} onClose={() => setUsageModalKey(null)} />
      )}
    </div>
  )
}
