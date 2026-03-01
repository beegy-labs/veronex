'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  usageAggregateQuery, analyticsQuery, performanceQuery,
  usageBreakdownQuery, keysQuery, keyUsageQuery,
} from '@/lib/queries'
import type { AnalyticsStats, UsageBreakdown } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import { DonutChart } from '@/components/donut-chart'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtMs,
} from '@/lib/chart-theme'
import { Hash, Coins, CheckCircle, XCircle, AlertTriangle, Zap, MessageSquare, Bot, Server, Key } from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Button } from '@/components/ui/button'
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

// fmtMs imported from chart-theme
const fmtLatency = fmtMs

const BACKEND_COLORS: Record<string, string> = {
  ollama: 'var(--theme-primary)',
  gemini: 'var(--theme-status-info)',
}
const BACKEND_BADGE: Record<string, string> = {
  ollama: 'bg-primary/10 text-primary border-primary/30',
  gemini: 'bg-status-info/10 text-status-info-fg border-status-info/30',
}

/* ─── Token composition donut ─────────────────────────────── */
function TokenDonut({ prompt, completion }: { prompt: number; completion: number }) {
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
          <DonutChart
            data={[
              { name: 'Prompt',     value: prompt,     fill: 'var(--theme-primary)' },
              { name: 'Completion', value: completion, fill: 'var(--theme-status-info)' },
            ]}
            size={160}
            innerRadius={44}
            outerRadius={68}
            formatter={fmt}
          />
          <div className="flex-1 space-y-4">
            {data.map((d, i) => (
              <div key={d.name}>
                <div className="flex items-center justify-between mb-1">
                  <div className="flex items-center gap-2">
                    <span className="inline-block h-2.5 w-2.5 rounded-full flex-shrink-0"
                      style={{ background: i === 0 ? 'var(--theme-primary)' : 'var(--theme-status-info)' }} />
                    <span className="text-xs font-bold uppercase tracking-widest text-muted-foreground">{d.name}</span>
                  </div>
                  <span className="text-sm font-mono font-bold">{d.pct}%</span>
                </div>
                <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                  <div className="h-full rounded-full transition-all"
                    style={{ width: `${d.pct}%`, background: i === 0 ? 'var(--theme-primary)' : 'var(--theme-status-info)' }} />
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

/* ─── Finish reason colors ────────────────────────────────── */
const FINISH_COLORS: Record<string, string> = {
  stop:      'var(--theme-status-success)',
  length:    'var(--theme-status-warning)',
  error:     'var(--theme-status-error)',
  cancelled: 'var(--theme-text-secondary)',
}
const FINISH_BG: Record<string, string> = {
  stop:      'bg-status-success/15 text-status-success-fg border-status-success/30',
  length:    'bg-status-warning/15 text-status-warning-fg border-status-warning/30',
  error:     'bg-status-error/15 text-status-error-fg border-status-error/30',
  cancelled: 'bg-muted text-muted-foreground border-border',
}

/* ─── Backend breakdown section ───────────────────────────── */
function BackendBreakdownSection({ data }: { data: UsageBreakdown }) {
  const { t } = useTranslation()
  if (data.by_backend.length === 0) return null
  const total = data.by_backend.reduce((s, b) => s + b.request_count, 0)

  return (
    <div className="space-y-3">
      <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground">{t('usage.byProvider')}</p>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        {data.by_backend.map((b) => {
          const pct = total > 0 ? Math.round((b.request_count / total) * 100) : 0
          const color = BACKEND_COLORS[b.backend] ?? 'var(--theme-primary)'
          const totalTok = b.prompt_tokens + b.completion_tokens
          return (
            <Card key={b.backend} className="overflow-hidden">
              <CardContent className="p-4 space-y-3">
                {/* header */}
                <div className="flex items-center justify-between">
                  <Badge variant="outline" className={`text-xs font-mono ${BACKEND_BADGE[b.backend] ?? ''}`}>
                    {b.backend}
                  </Badge>
                  <span className="text-2xl font-bold tabular-nums">{fmt(b.request_count)}</span>
                </div>
                {/* call ratio bar */}
                <div>
                  <div className="flex justify-between text-xs text-muted-foreground mb-1">
                    <span>{t('usage.callShare')}</span>
                    <span className="font-semibold tabular-nums" style={{ color }}>{pct}%</span>
                  </div>
                  <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                    <div className="h-full rounded-full transition-all" style={{ width: `${pct}%`, background: color }} />
                  </div>
                </div>
                {/* stats row */}
                <div className="grid grid-cols-3 gap-2 text-xs">
                  <div>
                    <p className="text-muted-foreground">Success</p>
                    <p className="font-semibold tabular-nums text-status-success-fg">{b.success_rate}%</p>
                  </div>
                  <div>
                    <p className="text-muted-foreground">Tokens</p>
                    <p className="font-semibold tabular-nums">{fmt(totalTok)}</p>
                  </div>
                  <div>
                    <p className="text-muted-foreground">Errors</p>
                    <p className={`font-semibold tabular-nums ${b.error_count > 0 ? 'text-status-error-fg' : 'text-muted-foreground'}`}>
                      {fmt(b.error_count)}
                    </p>
                  </div>
                </div>
              </CardContent>
            </Card>
          )
        })}
      </div>
    </div>
  )
}

/* ─── API Key breakdown section ───────────────────────────── */
function KeyBreakdownSection({ data }: { data: UsageBreakdown }) {
  const { t } = useTranslation()
  if (data.by_key.length === 0) return null
  const total = data.by_key.reduce((s, k) => s + k.request_count, 0)

  return (
    <div className="space-y-3">
      <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground">{t('usage.byKey')}</p>
      <DataTable minWidth="560px">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>Key</TableHead>
            <TableHead className="text-right w-24">Requests</TableHead>
            <TableHead className="text-right w-20">Share</TableHead>
            <TableHead className="text-right w-24">Success</TableHead>
            <TableHead className="text-right w-28">Tokens</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {data.by_key.map((k) => {
            const pct = total > 0 ? Math.round((k.request_count / total) * 100) : 0
            const totalTok = k.prompt_tokens + k.completion_tokens
            return (
              <TableRow key={k.key_id}>
                <TableCell>
                  <p className="font-semibold text-text-bright text-sm">{k.key_name}</p>
                  <p className="text-xs text-muted-foreground font-mono">{k.key_prefix}…</p>
                </TableCell>
                <TableCell className="text-right tabular-nums font-semibold">{fmt(k.request_count)}</TableCell>
                <TableCell className="text-right">
                  <div className="flex items-center justify-end gap-1.5">
                    <div className="h-1.5 w-16 rounded-full bg-muted overflow-hidden">
                      <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${pct}%` }} />
                    </div>
                    <span className="text-xs tabular-nums text-muted-foreground w-7 text-right">{pct}%</span>
                  </div>
                </TableCell>
                <TableCell className="text-right">
                  <span className={`text-sm font-semibold tabular-nums ${k.success_rate >= 90 ? 'text-status-success-fg' : k.success_rate >= 70 ? 'text-status-warning-fg' : 'text-status-error-fg'}`}>
                    {k.success_rate}%
                  </span>
                </TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmt(totalTok)}</TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </DataTable>
    </div>
  )
}

/* ─── Model breakdown section ─────────────────────────────── */
function ModelBreakdownSection({ data }: { data: UsageBreakdown }) {
  const { t } = useTranslation()
  if (data.by_model.length === 0) return null

  return (
    <div className="space-y-3">
      <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground">{t('usage.modelCallRatio')}</p>
      <DataTable minWidth="600px">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>Model</TableHead>
            <TableHead className="w-24">{t('usage.providerCol')}</TableHead>
            <TableHead className="text-right w-24">Requests</TableHead>
            <TableHead className="w-40">Call %</TableHead>
            <TableHead className="text-right w-28">Avg Latency</TableHead>
            <TableHead className="text-right w-28">Tokens</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {data.by_model.map((m, i) => {
            const totalTok = m.prompt_tokens + m.completion_tokens
            const color = BACKEND_COLORS[m.backend] ?? 'var(--theme-primary)'
            return (
              <TableRow key={`${m.model_name}-${m.backend}-${i}`}>
                <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
                <TableCell>
                  <Badge variant="outline" className={`text-xs ${BACKEND_BADGE[m.backend] ?? ''}`}>
                    {m.backend}
                  </Badge>
                </TableCell>
                <TableCell className="text-right tabular-nums font-semibold">{fmt(m.request_count)}</TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
                      <div className="h-full rounded-full transition-all" style={{ width: `${Math.min(m.call_pct, 100)}%`, background: color }} />
                    </div>
                    <span className="text-xs tabular-nums font-semibold w-10 text-right" style={{ color }}>
                      {m.call_pct.toFixed(1)}%
                    </span>
                  </div>
                </TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground text-sm">
                  {m.avg_latency_ms > 0 ? fmtLatency(m.avg_latency_ms) : '—'}
                </TableCell>
                <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmt(totalTok)}</TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </DataTable>
    </div>
  )
}

/* ─── Analytics section ───────────────────────────────────── */
function AnalyticsSection({ data, hours }: { data: AnalyticsStats; hours: number }) {
  const { t } = useTranslation()

  const totalRequests = data.finish_reasons.reduce((sum, r) => sum + r.count, 0)
  const donutData = data.finish_reasons.map((r) => ({
    name: r.reason,
    value: r.count,
    pct: totalRequests > 0 ? Math.round((r.count / totalRequests) * 100) : 0,
  }))

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold tracking-tight">{t('usage.analyticsTitle')}</h2>
        <p className="text-sm text-muted-foreground mt-0.5">{t('usage.analyticsDesc')}</p>
      </div>

      <div className="grid grid-cols-3 gap-4">
        <StatsCard title={t('usage.avgTps')} value={data.avg_tps > 0 ? `${data.avg_tps.toFixed(1)}` : '—'}
          subtitle={t('usage.avgTpsDesc')} icon={<Zap className="h-5 w-5" />} />
        <StatsCard title={t('usage.avgPromptTokens')} value={data.avg_prompt_tokens > 0 ? fmt(data.avg_prompt_tokens) : '—'}
          subtitle={t('usage.tokensPerReq')} icon={<MessageSquare className="h-5 w-5" />} />
        <StatsCard title={t('usage.avgCompletionTokens')} value={data.avg_completion_tokens > 0 ? fmt(data.avg_completion_tokens) : '—'}
          subtitle={t('usage.tokensPerReq')} icon={<Bot className="h-5 w-5" />} />
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-5 gap-4">
        <div className="xl:col-span-3 space-y-2">
          <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground">{t('usage.modelDistTitle')}</p>
          {data.models.length === 0 ? (
            <Card><CardContent className="py-8 text-center text-sm text-muted-foreground">{t('usage.noData')}</CardContent></Card>
          ) : (
            <DataTable minWidth="500px">
              <TableHeader>
                <TableRow>
                  <TableHead>{t('usage.modelName')}</TableHead>
                  <TableHead className="text-right w-24">{t('usage.reqCount')}</TableHead>
                  <TableHead className="text-right w-24">{t('usage.successRate')}</TableHead>
                  <TableHead className="text-right w-28">{t('usage.avgLatency')}</TableHead>
                  <TableHead className="text-right w-28">{t('usage.totalTok')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {data.models.map((m) => {
                  const pct = Math.round(m.success_rate * 100)
                  const totalTok = m.total_prompt_tokens + m.total_completion_tokens
                  return (
                    <TableRow key={m.model_name}>
                      <TableCell className="font-mono font-medium">{m.model_name}</TableCell>
                      <TableCell className="text-right tabular-nums">{fmt(m.request_count)}</TableCell>
                      <TableCell className="text-right tabular-nums">
                        <span className={pct >= 90 ? 'text-status-success-fg font-semibold' : pct >= 70 ? 'text-status-warning-fg' : 'text-status-error-fg'}>
                          {pct}%
                        </span>
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-muted-foreground">
                        {fmtMs(m.avg_latency_ms)}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-muted-foreground">{fmt(totalTok)}</TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </DataTable>
          )}
        </div>

        <div className="xl:col-span-2 space-y-2">
          <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground">{t('usage.finishReasonTitle')}</p>
          <Card>
            <CardContent className="pt-4">
              {donutData.length === 0 ? (
                <div className="py-6 text-center text-sm text-muted-foreground">{t('usage.noData')}</div>
              ) : (
                <div className="flex items-center gap-6">
                  <DonutChart
                    data={donutData.map((d) => ({
                      name: d.name,
                      value: d.value,
                      fill: FINISH_COLORS[d.name] ?? 'var(--theme-muted)',
                    }))}
                    size={120}
                    innerRadius={30}
                    outerRadius={50}
                    formatter={(v) => String(v)}
                  />
                  <div className="flex-1 space-y-2">
                    {donutData.map((d) => (
                      <div key={d.name} className="flex items-center justify-between gap-2">
                        <div className="flex items-center gap-2">
                          <span className="h-2 w-2 rounded-full shrink-0"
                            style={{ background: FINISH_COLORS[d.name] ?? 'var(--theme-muted)' }} />
                          <span className={`text-xs font-medium px-1.5 py-0.5 rounded border ${FINISH_BG[d.name] ?? 'bg-muted text-muted-foreground border-border'}`}>
                            {d.name}
                          </span>
                        </div>
                        <div className="text-right">
                          <span className="text-sm font-mono tabular-nums font-bold">{d.value}</span>
                          <span className="text-xs text-muted-foreground ml-1">({d.pct}%)</span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}

/* ─── page ────────────────────────────────────────────────── */
export default function UsagePage() {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)

  const { data: agg, isLoading: aggLoading, error: aggError } = useQuery(usageAggregateQuery(hours))
  const { data: analytics } = useQuery(analyticsQuery(hours))
  const { data: perf } = useQuery(performanceQuery(hours))
  const { data: breakdown } = useQuery(usageBreakdownQuery(hours))
  const { data: keys } = useQuery(keysQuery)

  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const activeKeyId = selectedKey ?? keys?.[0]?.id ?? null

  const { data: hourly, isLoading: hourlyLoading } = useQuery(keyUsageQuery(activeKeyId, hours))

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
    ? Math.round((agg.error_count / agg.request_count) * 100) : 0

  // Global trend from performance hourly data
  const globalTrendData = perf?.hourly.map((h) => ({
    hour:    fmtHour(h.hour),
    requests: h.request_count,
    tokens:   h.total_tokens,
  })) ?? []

  // Top 8 models bar chart
  const modelBarData = analytics?.models
    .slice()
    .sort((a, b) => b.request_count - a.request_count)
    .slice(0, 8)
    .map(m => ({ name: m.model_name, requests: m.request_count }))
    ?? []

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('usage.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('usage.description')}</p>
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          {TIME_OPTIONS.map((opt) => (
            <Button key={opt.hours} variant={hours === opt.hours ? 'default' : 'outline'} size="sm" onClick={() => setHours(opt.hours)}>
              {opt.label}
            </Button>
          ))}
        </div>
      </div>

      {aggError && (
        <Card className="border-status-warning/30 bg-status-warning/10">
          <CardContent className="p-5">
            <p className="font-semibold text-status-warning-fg">{t('usage.analyticsUnavailable')}</p>
            <p className="text-sm mt-1 text-status-warning-fg/80">{t('usage.clickhouseDisabled')}</p>
          </CardContent>
        </Card>
      )}

      {/* ── Aggregate KPI ──────────────────────────────── */}
      {agg && !aggError && (
        <>
          <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
            <StatsCard title={t('usage.totalRequests')} value={fmt(agg.request_count)}
              subtitle={`${t('common.last')} ${hours}h`} icon={<Hash className="h-5 w-5" />} />
            <StatsCard title={t('usage.totalTokens')} value={fmt(agg.total_tokens)}
              subtitle={`${fmt(agg.prompt_tokens)} prompt · ${fmt(agg.completion_tokens)} completion`}
              icon={<Coins className="h-5 w-5" />} />
            <StatsCard title={t('usage.success')}
              value={agg.request_count > 0 ? `${Math.round((agg.success_count / agg.request_count) * 100)}%` : '—'}
              subtitle={`${fmt(agg.success_count)} ${t('usage.completed')}`}
              icon={<CheckCircle className="h-5 w-5" />} />
            <StatsCard title={t('usage.errors')} value={fmt(agg.error_count)}
              subtitle={`${fmt(agg.cancelled_count)} ${t('usage.cancelled')}`}
              icon={errorRate >= 10
                ? <AlertTriangle className="h-5 w-5 text-[var(--theme-status-error)]" />
                : <XCircle className="h-5 w-5" />} />
          </div>

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

      {/* ── Global Request + Token Trend ──────────────── */}
      {globalTrendData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('performance.throughputHour')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('common.last')} {TIME_OPTIONS.find(o => o.hours === hours)?.label}</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={globalTrendData}>
                <defs>
                  <linearGradient id="gradReqs" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%"  stopColor="var(--theme-primary)" stopOpacity={0.25} />
                    <stop offset="95%" stopColor="var(--theme-primary)" stopOpacity={0} />
                  </linearGradient>
                  <linearGradient id="gradToks" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%"  stopColor="var(--theme-status-info)" stopOpacity={0.2} />
                    <stop offset="95%" stopColor="var(--theme-status-info)" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={45} tickFormatter={fmt} />
                <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v: number) => fmt(v)} />
                <Legend wrapperStyle={LEGEND_STYLE} />
                <Area type="monotone" dataKey="requests" name={t('usage.requests')}
                  stroke="var(--theme-primary)" fill="url(#gradReqs)" strokeWidth={2} dot={false} />
                <Area type="monotone" dataKey="tokens" name={t('usage.totalTokens')}
                  stroke="var(--theme-status-info)" fill="url(#gradToks)" strokeWidth={2} dot={false} />
              </AreaChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}

      {/* ── Model Distribution Bar Chart ───────────────── */}
      {modelBarData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('usage.modelDistTitle')}</CardTitle>
            <p className="text-xs text-muted-foreground">{t('common.last')} {TIME_OPTIONS.find(o => o.hours === hours)?.label}</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={Math.max(160, modelBarData.length * 36)}>
              <BarChart data={modelBarData} layout="vertical" margin={{ left: 8, right: 16 }}>
                <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={fmt} />
                <YAxis
                  type="category" dataKey="name" width={150}
                  tick={{ ...AXIS_TICK, fontSize: 10 }}
                  axisLine={false} tickLine={false}
                  tickFormatter={(v: string) => v.length > 22 ? v.slice(0, 21) + '…' : v}
                />
                <Tooltip
                  contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
                  cursor={CURSOR_FILL}
                  formatter={(v: number) => [fmt(v), t('usage.reqCount')]}
                />
                <Bar dataKey="requests" name={t('usage.reqCount')} fill="var(--theme-primary)" radius={[0, 4, 4, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}

      {/* ── Backend + Key + Model breakdown ───────────── */}
      {breakdown && (breakdown.by_backend.length > 0 || breakdown.by_key.length > 0) && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base flex items-center gap-2">
              <Server className="h-4 w-4 text-primary" />
              {t('usage.breakdownTitle')}
            </CardTitle>
            <p className="text-xs text-muted-foreground">{t('usage.breakdownDesc')}</p>
          </CardHeader>
          <CardContent className="space-y-8">
            <BackendBreakdownSection data={breakdown} />
            <KeyBreakdownSection data={breakdown} />
            <ModelBreakdownSection data={breakdown} />
          </CardContent>
        </Card>
      )}

      {/* ── Per-key hourly chart ───────────────────────── */}
      {!aggError && keys && keys.length > 0 && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between flex-wrap gap-3">
              <CardTitle className="text-base flex items-center gap-2">
                <Key className="h-4 w-4 text-primary" />
                {t('usage.hourly')}
              </CardTitle>
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
                      <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={45} tickFormatter={fmt} />
                      <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v: number) => fmt(v)} />
                      <Legend wrapperStyle={LEGEND_STYLE} />
                      <Area type="monotone" dataKey="prompt" name="Prompt"     stroke="var(--theme-primary)"       fill="url(#gradPrompt)" strokeWidth={2} dot={false} />
                      <Area type="monotone" dataKey="compl"  name="Completion" stroke="var(--theme-status-info)"  fill="url(#gradCompl)"  strokeWidth={2} dot={false} />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
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
                      <Bar dataKey="requests" name={t('usage.requests')} fill="var(--theme-primary)"              radius={[3, 3, 0, 0]} />
                      <Bar dataKey="success"  name={t('usage.success')}  fill="var(--theme-status-success)"      radius={[3, 3, 0, 0]} />
                      <Bar dataKey="errors"   name={t('usage.errors')}   fill="var(--theme-status-error)"        radius={[3, 3, 0, 0]} />
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

      {/* ── Analytics (ClickHouse) ─────────────────────── */}
      {analytics && !aggError && (
        <AnalyticsSection data={analytics} hours={hours} />
      )}
    </div>
  )
}
