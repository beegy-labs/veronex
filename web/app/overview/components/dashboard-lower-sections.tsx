'use client'

import Link from 'next/link'
import type { Job, UsageAggregate, ModelBreakdown } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar, Cell,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtMsNullable, fmtCompact,
} from '@/lib/chart-theme'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import { ArrowRight } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { fmtDatetimeShort } from '@/lib/date'
import { STATUS_STYLES, PROVIDER_GEMINI } from '@/lib/constants'
import { tokens } from '@/lib/design-tokens'

/* ─── Request Trend (24h area chart) ──────────────────────── */
export function RequestTrendSection({ trendData }: {
  trendData: { hour: string; total: number; success: number }[]
}) {
  const { t } = useTranslation()
  if (trendData.length === 0) return null

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('overview.requestTrend')}</CardTitle>
        <p className="text-xs text-muted-foreground">{t('overview.last24h')}</p>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={220}>
          <AreaChart data={trendData}>
            <defs>
              <linearGradient id="gradTotal" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%"  stopColor={tokens.brand.primary} stopOpacity={0.25} />
                <stop offset="95%" stopColor={tokens.brand.primary} stopOpacity={0} />
              </linearGradient>
              <linearGradient id="gradSuccess" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%"  stopColor={tokens.status.success} stopOpacity={0.2} />
                <stop offset="95%" stopColor={tokens.status.success} stopOpacity={0} />
              </linearGradient>
            </defs>
            <XAxis dataKey="hour" tick={AXIS_TICK} axisLine={false} tickLine={false} />
            <YAxis tick={AXIS_TICK} axisLine={false} tickLine={false} width={35} />
            <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} />
            <Legend wrapperStyle={LEGEND_STYLE} />
            <Area type="monotone" dataKey="total"   name={t('overview.totalReqs')}
              stroke={tokens.brand.primary} fill="url(#gradTotal)" strokeWidth={2} dot={false} />
            <Area type="monotone" dataKey="success" name={t('overview.successReqs')}
              stroke={tokens.status.success} fill="url(#gradSuccess)" strokeWidth={2} dot={false} />
          </AreaChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}

/* ─── Top Models (bar chart) ──────────────────────────────── */
export function TopModelsSection({ modelBarData, geminiEnabled }: {
  modelBarData: (ModelBreakdown & { label: string })[]
  geminiEnabled: boolean
}) {
  const { t } = useTranslation()
  if (modelBarData.length === 0) return null

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between flex-wrap gap-2">
          <div>
            <CardTitle>{t('overview.topModels')}</CardTitle>
            <p className="text-xs text-muted-foreground mt-0.5">{t('overview.last24h')}</p>
          </div>
          <div className="flex items-center gap-3 text-xs text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <span className="h-2.5 w-2.5 rounded-sm inline-block" style={{ background: tokens.brand.primary }} />
              {t('nav.ollama')}
            </span>
            {geminiEnabled && (
              <span className="flex items-center gap-1.5">
                <span className="h-2.5 w-2.5 rounded-sm inline-block" style={{ background: tokens.status.info }} />
                {t('nav.gemini')}
              </span>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={Math.max(160, modelBarData.length * 36)}>
          <BarChart data={modelBarData} layout="vertical" margin={{ left: 8, right: 16 }}>
            <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={fmtCompact} />
            <YAxis
              type="category" dataKey="label" width={154}
              tick={{ ...AXIS_TICK, fontSize: 10 }}
              axisLine={false} tickLine={false}
            />
            <Tooltip
              contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL}
              formatter={(v, _name, props: { payload?: ModelBreakdown }) => [
                `${fmtCompact(Number(v))} ${t('usage.reqCount')}`,
                props.payload?.provider_type ?? '',
              ] as [string, string]}
            />
            <Bar dataKey="request_count" radius={[0, 4, 4, 0]}>
              {modelBarData.map((m) => (
                <Cell key={`${m.model_name}-${m.provider_type}`} fill={m.provider_type === PROVIDER_GEMINI ? tokens.status.info : tokens.brand.primary} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}

/* ─── Recent Jobs ─────────────────────────────────────────── */
export function RecentJobsSection({ recentJobs, tz }: {
  recentJobs: Job[]
  tz: string
}) {
  const { t } = useTranslation()

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between pb-3">
        <CardTitle className="text-base">{t('overview.recentJobs')}</CardTitle>
        <Link href="/jobs" className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">
          {t('overview.viewAllJobs')} <ArrowRight className="h-3 w-3" />
        </Link>
      </CardHeader>
      {recentJobs.length === 0 ? (
        <CardContent className="pb-6 text-center text-sm text-muted-foreground">
          {t('jobs.noJobs')}
        </CardContent>
      ) : (
        <div className="overflow-x-auto">
          <Table style={{ minWidth: '560px' }} className="text-sm">
            <TableHeader>
              <TableRow className="border-b border-border">
                <TableHead className="h-11 px-4 pl-6 text-left text-xs font-medium text-muted-foreground">{t('jobs.model')}</TableHead>
                <TableHead className="h-11 px-4 text-left text-xs font-medium text-muted-foreground">{t('jobs.provider')}</TableHead>
                <TableHead className="h-11 px-4 text-left text-xs font-medium text-muted-foreground">{t('jobs.status')}</TableHead>
                <TableHead className="h-11 px-4 text-left text-xs font-medium text-muted-foreground">{t('jobs.latency')}</TableHead>
                <TableHead className="h-11 px-4 pr-6 text-left text-xs font-medium text-muted-foreground">{t('jobs.createdAt')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {recentJobs.map((job) => (
                <TableRow key={job.id} className="border-b border-border last:border-0">
                  <TableCell className="py-3 px-4 pl-6 font-mono text-xs max-w-[180px] truncate">{job.model_name}</TableCell>
                  <TableCell className="py-3 px-4 text-xs text-muted-foreground max-w-[120px] truncate">{job.provider_type}</TableCell>
                  <TableCell className="py-3 px-4">
                    <Badge variant="outline" className={`text-xs ${STATUS_STYLES[job.status] ?? 'bg-muted/20 text-muted-foreground border-muted/30'}`}>
                      {t(`jobs.statuses.${job.status}` as Parameters<typeof t>[0])}
                    </Badge>
                  </TableCell>
                  <TableCell className="py-3 px-4 text-xs tabular-nums">{fmtMsNullable(job.latency_ms)}</TableCell>
                  <TableCell className="py-3 px-4 pr-6 text-xs text-muted-foreground whitespace-nowrap">{fmtDatetimeShort(job.created_at, tz)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </Card>
  )
}

/* ─── Token Summary ───────────────────────────────────────── */
export function TokenSummarySection({ usage }: { usage: UsageAggregate | undefined }) {
  const { t } = useTranslation()

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-base">{t('overview.tokenSummary')}</CardTitle>
        <p className="text-xs text-muted-foreground">{t('overview.last24h')}</p>
      </CardHeader>
      <CardContent className="pt-0">
        {usage ? (
          <>
            <p className="text-3xl font-bold tabular-nums flex items-baseline gap-1">
              {fmtCompact(usage.total_tokens)}
              <span className="text-sm font-normal text-muted-foreground">{t('common.tokensUnit')}</span>
            </p>
            <p className="text-xs text-muted-foreground mt-1">
              {t('usage.promptTokens')} {fmtCompact(usage.prompt_tokens)} · {t('usage.completionTokens')} {fmtCompact(usage.completion_tokens)}
            </p>
          </>
        ) : (
          <p className="text-sm text-muted-foreground">{t('overview.analyticsOffline')}</p>
        )}
        <div className="mt-3 pt-2 border-t border-border">
          <Link href="/usage" className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors">
            {t('overview.goToUsage')} <ArrowRight className="h-3 w-3" />
          </Link>
        </div>
      </CardContent>
    </Card>
  )
}
