'use client'

import type { ModelBreakdown } from '@/lib/types'
import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, CURSOR_FILL,
} from '@/lib/chart-theme'
import { useTranslation } from '@/i18n'
import { PROVIDER_COLORS } from '@/lib/constants'

export function ModelLatencyChart({ data }: { data: ModelBreakdown[] }) {
  const { t } = useTranslation()
  const chartData = data
    .filter((m) => m.avg_latency_ms > 0)
    .sort((a, b) => b.avg_latency_ms - a.avg_latency_ms)
    .slice(0, 10)
    .map((m) => ({
      name: m.model_name.length > 24 ? m.model_name.slice(0, 23) + '…' : m.model_name,
      latency: Math.round(m.avg_latency_ms),
      color: PROVIDER_COLORS[m.provider_type] ?? 'var(--theme-primary)',
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
