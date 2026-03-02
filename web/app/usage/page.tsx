'use client'

import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  usageAggregateQuery, analyticsQuery, performanceQuery,
  usageBreakdownQuery, keysQuery, keyUsageQuery, keyModelBreakdownQuery,
} from '@/lib/queries'
import type { AnalyticsStats, ApiKey, ModelBreakdown, UsageBreakdown } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import { DonutChart } from '@/components/donut-chart'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtMs, fmtCompact,
} from '@/lib/chart-theme'
import {
  Hash, Coins, CheckCircle, XCircle, AlertTriangle, Zap,
  MessageSquare, Bot, Server, Key, BarChart2, DollarSign, Search,
} from 'lucide-react'
import StatsCard from '@/components/stats-card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select, SelectContent, SelectItem,
  SelectTrigger, SelectValue,
} from '@/components/ui/select'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { KeyUsageModal } from '@/components/key-usage-modal'
import { TIME_OPTIONS, TimeRangeSelector } from '@/components/time-range-selector'
import { fmtHourLabel } from '@/lib/date'
import { useTimezone } from '@/components/timezone-provider'

const fmtLatency = fmtMs

const BACKEND_COLORS: Record<string, string> = {
  ollama: 'var(--theme-primary)',
  gemini: 'var(--theme-status-info)',
}
const BACKEND_BADGE: Record<string, string> = {
  ollama: 'bg-primary/10 text-primary border-primary/30',
  gemini: 'bg-status-info/10 text-status-info-fg border-status-info/30',
}
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

/* ─── Token composition donut ─────────────────────────────── */
function TokenDonut({ prompt, completion }: { prompt: number; completion: number }) {
  const { t } = useTranslation()
  const total = prompt + completion
  if (total === 0) return null
  const data = [
    { name: t('usage.promptTokens'), value: prompt,     pct: Math.round((prompt / total) * 100) },
    { name: t('usage.completionTokens'), value: completion, pct: Math.round((completion / total) * 100) },
  ]
  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle className="text-base">Token Composition</CardTitle>
        <p className="text-xs text-muted-foreground">Prompt vs Completion token split</p>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-8">
          <DonutChart
            data={[
              { name: t('usage.promptTokens'),     value: prompt,     fill: 'var(--theme-primary)' },
              { name: t('usage.completionTokens'), value: completion, fill: 'var(--theme-status-info)' },
            ]}
            size={140}
            innerRadius={38}
            outerRadius={60}
            formatter={fmtCompact}
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
                <p className="text-xs text-muted-foreground mt-1">{fmtCompact(d.value)} tokens</p>
              </div>
            ))}
            <p className="text-xs text-muted-foreground pt-1 border-t border-border">
              Total <span className="font-bold text-foreground">{fmtCompact(total)}</span> tokens
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

/* ─── Finish reasons card ─────────────────────────────────── */
function FinishReasonsCard({ data }: { data: AnalyticsStats }) {
  const { t } = useTranslation()
  const total = data.finish_reasons.reduce((s, r) => s + r.count, 0)
  const donutData = data.finish_reasons.map((r) => ({
    name: r.reason,
    value: r.count,
    pct: total > 0 ? Math.round((r.count / total) * 100) : 0,
  }))
  if (donutData.length === 0) return null

  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle className="text-base">{t('usage.finishReasonTitle')}</CardTitle>
      </CardHeader>
      <CardContent>
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
      </CardContent>
    </Card>
  )
}

/* ─── Backend breakdown section ───────────────────────────── */
function BackendBreakdownSection({ data }: { data: UsageBreakdown }) {
  const { t } = useTranslation()
  if (data.by_backend.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )
  const total = data.by_backend.reduce((s, b) => s + b.request_count, 0)

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
      {data.by_backend.map((b) => {
        const pct = total > 0 ? Math.round((b.request_count / total) * 100) : 0
        const color = BACKEND_COLORS[b.backend] ?? 'var(--theme-primary)'
        const totalTok = b.prompt_tokens + b.completion_tokens
        return (
          <Card key={b.backend} className="overflow-hidden">
            <CardContent className="p-4 space-y-3">
              <div className="flex items-center justify-between">
                <Badge variant="outline" className={`text-xs font-mono ${BACKEND_BADGE[b.backend] ?? ''}`}>
                  {b.backend}
                </Badge>
                <span className="text-2xl font-bold tabular-nums">{fmtCompact(b.request_count)}</span>
              </div>
              <div>
                <div className="flex justify-between text-xs text-muted-foreground mb-1">
                  <span>{t('usage.callShare')}</span>
                  <span className="font-semibold tabular-nums" style={{ color }}>{pct}%</span>
                </div>
                <div className="h-1.5 rounded-full bg-muted overflow-hidden">
                  <div className="h-full rounded-full transition-all" style={{ width: `${pct}%`, background: color }} />
                </div>
              </div>
              <div className="grid grid-cols-3 gap-2 text-xs">
                <div>
                  <p className="text-muted-foreground">Success</p>
                  <p className="font-semibold tabular-nums text-status-success-fg">{b.success_rate}%</p>
                </div>
                <div>
                  <p className="text-muted-foreground">Tokens</p>
                  <p className="font-semibold tabular-nums">{fmtCompact(totalTok)}</p>
                </div>
                <div>
                  <p className="text-muted-foreground">Errors</p>
                  <p className={`font-semibold tabular-nums ${b.error_count > 0 ? 'text-status-error-fg' : 'text-muted-foreground'}`}>
                    {fmtCompact(b.error_count)}
                  </p>
                </div>
              </div>
              {b.estimated_cost_usd != null && (
                <div className="pt-2 border-t border-border text-xs flex justify-between items-center">
                  <span className="text-muted-foreground">{t('usage.estimatedCost')}</span>
                  <span className={`font-semibold tabular-nums font-mono ${b.estimated_cost_usd > 0 ? 'text-foreground' : 'text-muted-foreground'}`}>
                    {b.estimated_cost_usd === 0 ? 'Free' : `$${b.estimated_cost_usd.toFixed(4)}`}
                  </span>
                </div>
              )}
            </CardContent>
          </Card>
        )
      })}
    </div>
  )
}

/* ─── Key breakdown table ─────────────────────────────────── */
function KeyBreakdownTable({
  data,
  keys,
  selectedKeyId,
  onKeyClick,
  onKeySelect,
}: {
  data: UsageBreakdown
  keys: ApiKey[] | undefined
  selectedKeyId: string | null
  onKeyClick: (key: ApiKey) => void
  onKeySelect: (id: string) => void
}) {
  const { t } = useTranslation()
  if (data.by_key.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )
  const total = data.by_key.reduce((s, k) => s + k.request_count, 0)
  const keyMap = new Map(keys?.map((k) => [k.id, k]) ?? [])

  return (
    <DataTable minWidth="700px">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>Key</TableHead>
          <TableHead className="text-right w-24">Requests</TableHead>
          <TableHead className="w-32">Share</TableHead>
          <TableHead className="text-right w-24">Success</TableHead>
          <TableHead className="text-right w-28">Tokens</TableHead>
          <TableHead className="text-right w-28">{t('usage.estimatedCost')}</TableHead>
          <TableHead className="w-16" />
        </TableRow>
      </TableHeader>
      <TableBody>
        {data.by_key.map((k) => {
          const pct = total > 0 ? Math.round((k.request_count / total) * 100) : 0
          const totalTok = k.prompt_tokens + k.completion_tokens
          const apiKey = keyMap.get(k.key_id)
          const isSelected = selectedKeyId === k.key_id
          return (
            <TableRow
              key={k.key_id}
              className={`cursor-pointer transition-colors ${isSelected ? 'bg-primary/5 hover:bg-primary/8' : ''}`}
              onClick={() => onKeySelect(k.key_id)}
            >
              <TableCell>
                <div className="flex items-center gap-2">
                  {isSelected && <span className="h-1.5 w-1.5 rounded-full bg-primary shrink-0" />}
                  <div>
                    <p className="font-semibold text-text-bright text-sm">{k.key_name}</p>
                    <p className="text-xs text-muted-foreground font-mono">{k.key_prefix}…</p>
                  </div>
                </div>
              </TableCell>
              <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(k.request_count)}</TableCell>
              <TableCell>
                <div className="flex items-center gap-1.5">
                  <div className="flex-1 h-1.5 rounded-full bg-muted overflow-hidden">
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
              <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmtCompact(totalTok)}</TableCell>
              <TableCell className="text-right tabular-nums text-sm font-mono">
                {k.estimated_cost_usd == null
                  ? <span className="text-muted-foreground">—</span>
                  : k.estimated_cost_usd === 0
                    ? <span className="text-muted-foreground">Free</span>
                    : <span className="text-foreground">${k.estimated_cost_usd.toFixed(4)}</span>}
              </TableCell>
              <TableCell className="text-right" onClick={(e) => e.stopPropagation()}>
                {apiKey && (
                  <Button variant="ghost" size="icon" className="h-7 w-7 text-muted-foreground hover:text-primary"
                    onClick={() => onKeyClick(apiKey)} title={t('keys.viewUsage')}>
                    <BarChart2 className="h-3.5 w-3.5" />
                  </Button>
                )}
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </DataTable>
  )
}

/* ─── Model breakdown table (filterable) ─────────────────── */
function ModelBreakdownTable({
  data,
  filter,
}: {
  data: ModelBreakdown[]
  filter: string
}) {
  const { t } = useTranslation()
  const filtered = filter.trim()
    ? data.filter((m) => m.model_name.toLowerCase().includes(filter.toLowerCase()))
    : data

  if (filtered.length === 0) return (
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )

  return (
    <DataTable minWidth="760px">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>Model</TableHead>
          <TableHead className="w-28">{t('usage.providerCol')}</TableHead>
          <TableHead className="text-right w-24">Requests</TableHead>
          <TableHead className="w-40">Call %</TableHead>
          <TableHead className="text-right w-32">Avg Latency</TableHead>
          <TableHead className="text-right w-28">Tokens</TableHead>
          <TableHead className="text-right w-28">{t('usage.estimatedCost')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {filtered.map((m, i) => {
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
              <TableCell className="text-right tabular-nums font-semibold">{fmtCompact(m.request_count)}</TableCell>
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
              <TableCell className="text-right tabular-nums text-muted-foreground text-sm">{fmtCompact(totalTok)}</TableCell>
              <TableCell className="text-right tabular-nums text-sm font-mono">
                {m.estimated_cost_usd == null
                  ? <span className="text-muted-foreground">—</span>
                  : m.estimated_cost_usd === 0
                    ? <span className="text-muted-foreground">Free</span>
                    : <span className="text-foreground">${m.estimated_cost_usd.toFixed(4)}</span>}
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </DataTable>
  )
}

/* ─── Model latency bar chart ────────────────────────────── */
function ModelLatencyChart({ data }: { data: ModelBreakdown[] }) {
  const { t } = useTranslation()
  const chartData = data
    .filter((m) => m.avg_latency_ms > 0)
    .sort((a, b) => b.avg_latency_ms - a.avg_latency_ms)
    .slice(0, 10)
    .map((m) => ({
      name: m.model_name.length > 24 ? m.model_name.slice(0, 23) + '…' : m.model_name,
      latency: Math.round(m.avg_latency_ms),
      color: BACKEND_COLORS[m.backend] ?? 'var(--theme-primary)',
    }))

  if (chartData.length === 0) return null

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t('usage.modelLatencyChart')}</CardTitle>
        <p className="text-xs text-muted-foreground">{t('usage.avgLatency')} per model (ms)</p>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={Math.max(140, chartData.length * 34)}>
          <BarChart data={chartData} layout="vertical" margin={{ left: 8, right: 24 }}>
            <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={(v) => `${v}ms`} />
            <YAxis
              type="category" dataKey="name" width={160}
              tick={{ ...AXIS_TICK, fontSize: 10 }}
              axisLine={false} tickLine={false}
            />
            <Tooltip
              contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE}
              cursor={CURSOR_FILL}
              formatter={(v) => [`${v}ms`, t('usage.avgLatency')] as [string, string]}
            />
            <Bar dataKey="latency" name={t('usage.avgLatency')} fill="var(--theme-status-info)" radius={[0, 4, 4, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}

/* ─── page ────────────────────────────────────────────────── */
export default function UsagePage() {
  const { t } = useTranslation()
  const { tz } = useTimezone()
  const [hours, setHours] = useState(24)

  const { data: agg, isLoading: aggLoading, error: aggError } = useQuery(usageAggregateQuery(hours))
  const { data: analytics } = useQuery(analyticsQuery(hours))
  const { data: perf } = useQuery(performanceQuery(hours))
  const { data: breakdown } = useQuery(usageBreakdownQuery(hours))
  const { data: keys } = useQuery(keysQuery)

  // per-key hourly state
  const [selectedKeyId, setSelectedKeyId] = useState<string | null>(null)
  const activeKeyId = selectedKeyId ?? keys?.[0]?.id ?? null
  const [usageModalKey, setUsageModalKey] = useState<ApiKey | null>(null)
  const [modelFilter, setModelFilter] = useState('')

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

  const errorRate = agg && agg.request_count > 0
    ? Math.round((agg.error_count / agg.request_count) * 100) : 0

  const globalTrendData = perf?.hourly.map((h) => ({
    hour:     fmtHourLabel(h.hour, tz),
    requests: h.request_count,
    tokens:   h.total_tokens,
  })) ?? []

  const currentLabel = TIME_OPTIONS.find(o => o.hours === hours)?.label ?? `${hours}h`

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t('usage.title')}</h1>
          <p className="text-muted-foreground mt-1 text-sm">{t('usage.description')}</p>
        </div>
        <TimeRangeSelector value={hours} onChange={setHours} />
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
            value={agg.request_count > 0 ? `${Math.round((agg.success_count / agg.request_count) * 100)}%` : '—'}
            subtitle={`${fmtCompact(agg.success_count)} ${t('usage.completed')}`}
            icon={<CheckCircle className="h-5 w-5" />} />
          <StatsCard title={t('usage.errors')} value={fmtCompact(agg.error_count)}
            subtitle={`${fmtCompact(agg.cancelled_count)} ${t('usage.cancelled')}`}
            icon={errorRate >= 10
              ? <AlertTriangle className="h-5 w-5 text-[var(--theme-status-error)]" />
              : <XCircle className="h-5 w-5" />} />
        </div>
      )}

      {/* Total cost badge */}
      {breakdown && breakdown.total_cost_usd > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/30 px-4 py-2 w-fit">
          <DollarSign className="h-4 w-4 text-muted-foreground" />
          <div>
            <p className="text-[10px] uppercase tracking-widest text-muted-foreground font-bold">{t('usage.totalCost')}</p>
            <p className="text-lg font-bold tabular-nums font-mono">${breakdown.total_cost_usd.toFixed(4)}</p>
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
        <TabsContent value="overview" className="space-y-6 mt-4">
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
                        <stop offset="5%"  stopColor="var(--theme-primary)" stopOpacity={0.25} />
                        <stop offset="95%" stopColor="var(--theme-primary)" stopOpacity={0} />
                      </linearGradient>
                      <linearGradient id="gradToks" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%"  stopColor="var(--theme-status-info)" stopOpacity={0.2} />
                        <stop offset="95%" stopColor="var(--theme-status-info)" stopOpacity={0} />
                      </linearGradient>
                    </defs>
                    <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
                    <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={45} tickFormatter={fmtCompact} />
                    <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v) => fmtCompact(Number(v))} />
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
                  <div className="grid grid-cols-3 gap-3">
                    <div className="rounded-lg border border-border p-3 text-center">
                      <Zap className="h-4 w-4 mx-auto mb-1.5 text-muted-foreground" />
                      <p className="text-xl font-bold tabular-nums">
                        {analytics.avg_tps > 0 ? analytics.avg_tps.toFixed(1) : '—'}
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
          {analytics && analytics.models.length > 0 && (
            <Card>
              <CardHeader>
                <CardTitle className="text-base">{t('usage.modelDistTitle')}</CardTitle>
                <p className="text-xs text-muted-foreground">{t('common.last')} {currentLabel}</p>
              </CardHeader>
              <CardContent>
                <ResponsiveContainer width="100%" height={Math.max(160, analytics.models.slice(0, 8).length * 36)}>
                  <BarChart
                    data={analytics.models.slice().sort((a, b) => b.request_count - a.request_count).slice(0, 8).map(m => ({ name: m.model_name, requests: m.request_count }))}
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
                    <Bar dataKey="requests" name={t('usage.reqCount')} fill="var(--theme-primary)" radius={[0, 4, 4, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        {/* ── By Key ──────────────────────────────────── */}
        <TabsContent value="by-key" className="space-y-6 mt-4">
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
                              <Area type="monotone" dataKey="prompt" name="Prompt"     stroke="var(--theme-primary)"      fill="url(#gradPrompt)" strokeWidth={2} dot={false} />
                              <Area type="monotone" dataKey="compl"  name="Completion" stroke="var(--theme-status-info)"  fill="url(#gradCompl)"  strokeWidth={2} dot={false} />
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
                              <TableHead>Model</TableHead>
                              <TableHead className="w-28">{t('usage.providerCol')}</TableHead>
                              <TableHead className="text-right w-24">Requests</TableHead>
                              <TableHead className="w-36">Share</TableHead>
                              <TableHead className="text-right w-32">Avg Latency</TableHead>
                              <TableHead className="text-right w-28">Tokens</TableHead>
                            </TableRow>
                          </TableHeader>
                          <TableBody>
                            {keyModels.map((m, i) => {
                              const totalTok = m.prompt_tokens + m.completion_tokens
                              const color = BACKEND_COLORS[m.backend] ?? 'var(--theme-primary)'
                              return (
                                <TableRow key={`${m.model_name}-${i}`}>
                                  <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
                                  <TableCell>
                                    <Badge variant="outline" className={`text-xs ${BACKEND_BADGE[m.backend] ?? ''}`}>
                                      {m.backend}
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
                                    {m.avg_latency_ms > 0 ? fmtLatency(m.avg_latency_ms) : '—'}
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
            <p className="text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-4">
              {t('usage.byProvider')}
            </p>
            {!breakdown && (
              <div className="flex h-32 items-center justify-center text-muted-foreground text-sm">{t('common.loading')}</div>
            )}
            {breakdown && <BackendBreakdownSection data={breakdown} />}
          </div>
        </TabsContent>
      </Tabs>

      {usageModalKey && (
        <KeyUsageModal apiKey={usageModalKey} onClose={() => setUsageModalKey(null)} />
      )}
    </div>
  )
}
