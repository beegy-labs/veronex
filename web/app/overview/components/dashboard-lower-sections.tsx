'use client'

import Link from 'next/link'
import type { UsageAggregate, ModelBreakdown } from '@/lib/types'
import {
  AreaChart, Area, BarChart, Bar, Cell,
  XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, LEGEND_STYLE, CURSOR_FILL,
  fmtCompact,
} from '@/lib/chart-theme'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ArrowRight } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { PROVIDER_GEMINI } from '@/lib/constants'
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
        <p className="vds-text-xs vds-text-dim">{t('overview.last24h')}</p>
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
        <div className="vds-flex vds-items-center vds-justify-between vds-flex-wrap vds-gap-2">
          <div>
            <CardTitle>{t('overview.topModels')}</CardTitle>
            <p className="vds-text-xs vds-text-dim vds-mt-0.5">{t('overview.last24h')}</p>
          </div>
          <div className="vds-flex vds-items-center vds-gap-3 vds-text-xs vds-text-dim">
            <span className="vds-flex vds-items-center vds-gap-1.5">
              <span className="vds-h-2.5 vds-w-2.5 vds-rounded-sm vds-inline-block" style={{ background: tokens.brand.primary }} />
              {t('nav.llama_server')}
            </span>
            {geminiEnabled && (
              <span className="vds-flex vds-items-center vds-gap-1.5">
                <span className="vds-h-2.5 vds-w-2.5 vds-rounded-sm vds-inline-block" style={{ background: tokens.status.info }} />
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

/* ─── Token Summary ───────────────────────────────────────── */
export function TokenSummarySection({ usage }: { usage: UsageAggregate | undefined }) {
  const { t } = useTranslation()

  return (
    <Card>
      <CardHeader className="vds-pb-2">
        <CardTitle className="vds-text-base">{t('overview.tokenSummary')}</CardTitle>
        <p className="vds-text-xs vds-text-dim">{t('overview.last24h')}</p>
      </CardHeader>
      <CardContent className="vds-pt-0">
        {usage ? (
          <>
            <p className="vds-text-3xl vds-font-700 vds-tabular-nums vds-flex vds-items-baseline vds-gap-1">
              {fmtCompact(usage.total_tokens)}
              <span className="vds-text-sm vds-font-400 vds-text-dim">{t('common.tokensUnit')}</span>
            </p>
            <p className="vds-text-xs vds-text-dim vds-mt-1">
              {t('usage.promptTokens')} {fmtCompact(usage.prompt_tokens)} · {t('usage.completionTokens')} {fmtCompact(usage.completion_tokens)}
            </p>
          </>
        ) : (
          <p className="vds-text-sm vds-text-dim">{t('overview.analyticsOffline')}</p>
        )}
        <div className="vds-mt-3 vds-pt-2 vds-border-t-1 vds-border-subtle">
          <Link href="/usage" className="vds-text-xs vds-text-dim vds-hover:text-primary vds-flex vds-items-center vds-gap-1 vds-transition-colors">
            {t('overview.goToUsage')} <ArrowRight className="vds-h-3 vds-w-3" />
          </Link>
        </div>
      </CardContent>
    </Card>
  )
}
