'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { keyUsageQuery } from '@/lib/queries'
import type { ApiKey } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
} from '@/lib/chart-theme'
import { Hash, Coins, CheckCircle, XCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import StatsCard from '@/components/stats-card'
import { useTranslation } from '@/i18n'

const TIME_OPTIONS = [
  { label: '24h', hours: 24 },
  { label: '7d',  hours: 168 },
  { label: '30d', hours: 720 },
]

function fmt(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

function fmtHour(iso: string) {
  const d = new Date(iso)
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}h`
}

export function KeyUsageModal({
  apiKey,
  onClose,
}: {
  apiKey: ApiKey
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)

  const { data: hourly, isLoading } = useQuery(keyUsageQuery(apiKey.id, hours))

  const chartData = hourly?.map((h) => ({
    hour:     fmtHour(h.hour),
    tokens:   h.total_tokens,
    prompt:   h.prompt_tokens,
    compl:    h.completion_tokens,
    requests: h.request_count,
    success:  h.success_count,
    errors:   h.error_count,
  })) ?? []

  // Aggregate KPIs from hourly data
  const totalRequests = chartData.reduce((s, h) => s + h.requests, 0)
  const totalTokens   = chartData.reduce((s, h) => s + h.tokens, 0)
  const totalSuccess  = chartData.reduce((s, h) => s + h.success, 0)
  const totalErrors   = chartData.reduce((s, h) => s + h.errors, 0)
  const successRate   = totalRequests > 0
    ? Math.round((totalSuccess / totalRequests) * 100) : 0

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-center justify-between gap-3 flex-wrap">
            <div>
              <DialogTitle className="text-lg">
                {t('keys.usageTitle', { name: apiKey.name })}
              </DialogTitle>
              <div className="flex items-center gap-2 mt-1">
                <code className="text-xs font-mono text-muted-foreground">{apiKey.key_prefix}…</code>
                <Badge
                  variant="outline"
                  className={
                    apiKey.tier === 'free'
                      ? 'text-muted-foreground border-border text-[10px]'
                      : 'bg-status-info/10 text-status-info-fg border-status-info/30 text-[10px]'
                  }
                >
                  {apiKey.tier === 'free' ? t('keys.tierFree') : t('keys.tierPaid')}
                </Badge>
              </div>
            </div>
            <div className="flex items-center gap-1">
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
        </DialogHeader>

        {isLoading && (
          <div className="flex h-48 items-center justify-center text-muted-foreground text-sm">
            {t('common.loading')}
          </div>
        )}

        {!isLoading && (
          <div className="space-y-6 mt-2">
            {/* KPI row */}
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              <StatsCard
                title={t('usage.totalRequests')}
                value={fmt(totalRequests)}
                icon={<Hash className="h-4 w-4" />}
              />
              <StatsCard
                title={t('usage.totalTokens')}
                value={fmt(totalTokens)}
                icon={<Coins className="h-4 w-4" />}
              />
              <StatsCard
                title={t('usage.success')}
                value={totalRequests > 0 ? `${successRate}%` : '—'}
                icon={<CheckCircle className="h-4 w-4" />}
              />
              <StatsCard
                title={t('usage.errors')}
                value={fmt(totalErrors)}
                icon={<XCircle className="h-4 w-4" />}
              />
            </div>

            {chartData.length === 0 ? (
              <div className="flex h-32 items-center justify-center text-muted-foreground text-sm rounded-lg border border-dashed">
                {t('usage.noKeyData')}
              </div>
            ) : (
              <>
                {/* Token chart */}
                <div>
                  <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
                    {t('usage.tokensPerHour')}
                  </p>
                  <ResponsiveContainer width="100%" height={180}>
                    <AreaChart data={chartData}>
                      <defs>
                        <linearGradient id="ku-gradPrompt" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%"  stopColor="var(--theme-primary)" stopOpacity={0.35} />
                          <stop offset="95%" stopColor="var(--theme-primary)" stopOpacity={0} />
                        </linearGradient>
                        <linearGradient id="ku-gradCompl" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%"  stopColor="var(--theme-status-info)" stopOpacity={0.3} />
                          <stop offset="95%" stopColor="var(--theme-status-info)" stopOpacity={0} />
                        </linearGradient>
                      </defs>
                      <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={42} tickFormatter={fmt} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v) => fmt(Number(v))} />
                      <Legend wrapperStyle={LEGEND_STYLE} />
                      <Area type="monotone" dataKey="prompt" name="Prompt"     stroke="var(--theme-primary)"       fill="url(#ku-gradPrompt)" strokeWidth={2} dot={false} />
                      <Area type="monotone" dataKey="compl"  name="Completion" stroke="var(--theme-status-info)"  fill="url(#ku-gradCompl)"  strokeWidth={2} dot={false} />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>

                {/* Request chart */}
                <div>
                  <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
                    {t('usage.requestsPerHour')}
                  </p>
                  <ResponsiveContainer width="100%" height={160}>
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
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
