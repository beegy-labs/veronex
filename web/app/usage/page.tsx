'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import {
  AreaChart, Area, BarChart, Bar,
  PieChart, Pie, Cell,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import { Hash, Coins, CheckCircle, XCircle, AlertTriangle } from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select, SelectContent, SelectItem,
  SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { useTranslation } from '@/i18n'

const HOUR_OPTIONS = [6, 12, 24, 48, 72]

const TOOLTIP_STYLE = {
  backgroundColor: 'var(--theme-bg-card)',
  border: '1px solid var(--theme-border)',
  borderRadius: '8px',
  color: 'var(--theme-text-primary)',
  fontSize: '12px',
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

/* ─── Token composition donut ─────────────────────────────── */
function TokenDonut({
  prompt,
  completion,
}: {
  prompt: number
  completion: number
}) {
  const total = prompt + completion
  if (total === 0) return null

  const data = [
    { name: 'Prompt',     value: prompt,     pct: Math.round((prompt / total) * 100) },
    { name: 'Completion', value: completion, pct: Math.round((completion / total) * 100) },
  ]

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Token Composition</CardTitle>
        <p className="text-xs text-muted-foreground">Prompt vs Completion token split</p>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-8">
          {/* donut */}
          <ResponsiveContainer width={160} height={160}>
            <PieChart>
              <Pie
                data={data}
                dataKey="value"
                cx="50%" cy="50%"
                innerRadius={44} outerRadius={68}
                strokeWidth={0}
              >
                <Cell fill="var(--theme-primary)" />
                <Cell fill="var(--theme-status-info)" />
              </Pie>
              <Tooltip
                contentStyle={TOOLTIP_STYLE}
                formatter={(v: number) => [fmt(v), '']}
              />
            </PieChart>
          </ResponsiveContainer>

          {/* legend + numbers */}
          <div className="flex-1 space-y-4">
            {data.map((d, i) => (
              <div key={d.name}>
                <div className="flex items-center justify-between mb-1">
                  <div className="flex items-center gap-2">
                    <span
                      className="inline-block h-2.5 w-2.5 rounded-full flex-shrink-0"
                      style={{ background: i === 0 ? 'var(--theme-primary)' : 'var(--theme-status-info)' }}
                    />
                    <span className="text-xs font-bold uppercase tracking-widest text-muted-foreground">
                      {d.name}
                    </span>
                  </div>
                  <span className="text-sm font-mono font-bold">{d.pct}%</span>
                </div>
                {/* progress bar */}
                <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all"
                    style={{
                      width: `${d.pct}%`,
                      background: i === 0 ? 'var(--theme-primary)' : 'var(--theme-status-info)',
                    }}
                  />
                </div>
                <p className="text-xs text-muted-foreground mt-1">{fmt(d.value)} tokens</p>
              </div>
            ))}
            <p className="text-xs text-muted-foreground pt-1 border-t border-border">
              Total <span className="font-bold text-foreground">{fmt(total)}</span> tokens
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

/* ─── page ────────────────────────────────────────────────── */
export default function UsagePage() {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)

  const { data: agg, isLoading: aggLoading, error: aggError } = useQuery({
    queryKey: ['usage-aggregate', hours],
    queryFn: () => api.usageAggregate(hours),
    refetchInterval: 60_000,
  })

  const { data: keys } = useQuery({
    queryKey: ['keys'],
    queryFn: () => api.keys(),
    staleTime: 120_000,
  })

  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const activeKeyId = selectedKey ?? keys?.[0]?.id ?? null

  const { data: hourly, isLoading: hourlyLoading } = useQuery({
    queryKey: ['key-usage', activeKeyId, hours],
    queryFn: () => api.keyUsage(activeKeyId!, hours),
    enabled: !!activeKeyId,
    refetchInterval: 60_000,
  })

  const chartData = hourly?.map((h) => ({
    hour:     fmtHour(h.hour),
    tokens:   h.total_tokens,
    prompt:   h.prompt_tokens,
    compl:    h.completion_tokens,
    requests: h.request_count,
    success:  h.success_count,
    errors:   h.error_count,
  })) ?? []

  const errorRate = agg && agg.request_count > 0
    ? Math.round((agg.error_count / agg.request_count) * 100)
    : 0

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">
            {t('usage.title')}
          </h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('usage.description')}</p>
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-sm text-muted-foreground">{t('common.last')}</span>
          {HOUR_OPTIONS.map((h) => (
            <Button key={h} variant={hours === h ? 'default' : 'outline'} size="sm" onClick={() => setHours(h)}>
              {h}h
            </Button>
          ))}
        </div>
      </div>

      {/* ClickHouse unavailable */}
      {aggError && (
        <Card className="border-status-warning/30 bg-status-warning/10">
          <CardContent className="p-5">
            <p className="font-semibold text-status-warning-fg">{t('usage.analyticsUnavailable')}</p>
            <p className="text-sm mt-1 text-status-warning-fg/80">{t('usage.clickhouseDisabled')}</p>
          </CardContent>
        </Card>
      )}

      {/* ── Aggregate KPI cards ───────────────────────────── */}
      {agg && !aggError && (
        <>
          <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
            <StatsCard
              title={t('usage.totalRequests')}
              value={fmt(agg.request_count)}
              subtitle={`${t('common.last')} ${hours}h`}
              icon={<Hash className="h-5 w-5" />}
            />
            <StatsCard
              title={t('usage.totalTokens')}
              value={fmt(agg.total_tokens)}
              subtitle={`${fmt(agg.prompt_tokens)} prompt · ${fmt(agg.completion_tokens)} completion`}
              icon={<Coins className="h-5 w-5" />}
            />
            <StatsCard
              title={t('usage.success')}
              value={agg.request_count > 0
                ? `${Math.round((agg.success_count / agg.request_count) * 100)}%`
                : '—'}
              subtitle={`${fmt(agg.success_count)} ${t('usage.completed')}`}
              icon={<CheckCircle className="h-5 w-5" />}
            />
            <StatsCard
              title={t('usage.errors')}
              value={fmt(agg.error_count)}
              subtitle={`${fmt(agg.cancelled_count)} ${t('usage.cancelled')}`}
              icon={errorRate >= 10
                ? <AlertTriangle className="h-5 w-5 text-[var(--theme-status-error)]" />
                : <XCircle className="h-5 w-5" />}
            />
          </div>

          {/* ── Token composition donut ────────────────────── */}
          {agg.total_tokens > 0 && (
            <TokenDonut prompt={agg.prompt_tokens} completion={agg.completion_tokens} />
          )}

          {agg.request_count === 0 && (
            <Card>
              <CardContent className="p-10 text-center text-muted-foreground">
                <p className="font-medium">{t('usage.noData')}</p>
                <p className="text-sm mt-1">{t('usage.noDataHint')}</p>
              </CardContent>
            </Card>
          )}
        </>
      )}

      {/* ── Per-key hourly breakdown ──────────────────────── */}
      {!aggError && keys && keys.length > 0 && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between flex-wrap gap-3">
              <CardTitle className="text-base">{t('usage.hourly')}</CardTitle>
              <Select value={activeKeyId ?? ''} onValueChange={setSelectedKey}>
                <SelectTrigger className="w-56">
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
          <CardContent>
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
              <div className="space-y-8">
                {/* Token area chart: prompt + completion stacked */}
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
                      <XAxis dataKey="hour" tick={{ fill: 'var(--theme-text-secondary)', fontSize: 11 }} axisLine={false} tickLine={false} />
                      <YAxis tick={{ fill: 'var(--theme-text-secondary)', fontSize: 11 }} axisLine={false} tickLine={false} width={45} tickFormatter={fmt} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} cursor={{ fill: 'var(--theme-bg-hover)' }} formatter={(v: number) => [fmt(v), '']} />
                      <Legend wrapperStyle={{ fontSize: '12px', color: 'var(--theme-text-secondary)' }} />
                      <Area type="monotone" dataKey="prompt" name="Prompt"     stroke="var(--theme-primary)"              fill="url(#gradPrompt)" strokeWidth={2} dot={false} />
                      <Area type="monotone" dataKey="compl"  name="Completion" stroke="var(--theme-status-info)"    fill="url(#gradCompl)"  strokeWidth={2} dot={false} />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>

                {/* Request / success / error bar chart */}
                <div>
                  <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3">
                    {t('usage.requestsPerHour')}
                  </p>
                  <ResponsiveContainer width="100%" height={180}>
                    <BarChart data={chartData} barGap={2}>
                      <XAxis dataKey="hour" tick={{ fill: 'var(--theme-text-secondary)', fontSize: 11 }} axisLine={false} tickLine={false} />
                      <YAxis tick={{ fill: 'var(--theme-text-secondary)', fontSize: 11 }} axisLine={false} tickLine={false} width={35} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} cursor={{ fill: 'var(--theme-bg-hover)' }} />
                      <Legend wrapperStyle={{ fontSize: '12px', color: 'var(--theme-text-secondary)' }} />
                      <Bar dataKey="requests" name={t('usage.requests')} fill="var(--theme-primary)"                 radius={[3, 3, 0, 0]} />
                      <Bar dataKey="success"  name={t('usage.success')}  fill="var(--theme-status-success)"  radius={[3, 3, 0, 0]} />
                      <Bar dataKey="errors"   name={t('usage.errors')}   fill="var(--theme-status-error)"    radius={[3, 3, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {!aggError && !aggLoading && (!keys || keys.length === 0) && (
        <Card>
          <CardContent className="p-6 text-center text-muted-foreground text-sm">
            {t('usage.noKeysMsg')}
          </CardContent>
        </Card>
      )}
    </div>
  )
}
