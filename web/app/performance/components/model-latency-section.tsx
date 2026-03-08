'use client'

import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer,
} from 'recharts'
import {
  TOOLTIP_STYLE, TOOLTIP_LABEL_STYLE, TOOLTIP_ITEM_STYLE,
  AXIS_TICK, CURSOR_FILL, fmtMs, fmtCompact, fmtPct,
} from '@/lib/chart-theme'
import { Bot } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { DataTable } from '@/components/data-table'
import { useTranslation } from '@/i18n'
import { PROVIDER_BADGE } from '@/lib/constants'

interface ModelPerfRow {
  model_name: string
  provider_type: string
  request_count: number
  avg_latency_ms: number
  success_rate?: number
}

export function ModelLatencySection({ models }: { models: ModelPerfRow[] }) {
  const { t } = useTranslation()
  if (models.length === 0) return null

  const chartData = models
    .filter((m) => m.avg_latency_ms > 0)
    .sort((a, b) => a.avg_latency_ms - b.avg_latency_ms)
    .slice(0, 10)
    .map((m) => ({
      name: m.model_name.length > 24 ? m.model_name.slice(0, 23) + '…' : m.model_name,
      latency: Math.round(m.avg_latency_ms),
    }))

  return (
    <div className="grid grid-cols-1 xl:grid-cols-5 gap-4">
      <Card className="xl:col-span-3">
        <CardHeader>
          <CardTitle className="text-base flex items-center gap-2">
            <Bot className="h-4 w-4 text-primary" />
            {t('performance.byModel')}
          </CardTitle>
          <p className="text-xs text-muted-foreground">{t('performance.modelLatency')}</p>
        </CardHeader>
        <CardContent>
          <DataTable minWidth="500px">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>{t('usage.modelCol')}</TableHead>
                <TableHead className="w-28">{t('usage.providerCol')}</TableHead>
                <TableHead className="text-right w-24">{t('usage.requestsCol')}</TableHead>
                <TableHead className="text-right w-32">{t('usage.avgLatencyCol')}</TableHead>
                <TableHead className="text-right w-24">{t('usage.successCol')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {models.map((m, i) => (
                <TableRow key={`${m.model_name}-${m.provider_type}-${i}`}>
                  <TableCell className="font-mono font-medium text-sm">{m.model_name}</TableCell>
                  <TableCell>
                    <Badge variant="outline" className={`text-xs ${PROVIDER_BADGE[m.provider_type] ?? ''}`}>
                      {m.provider_type}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{fmtCompact(m.request_count)}</TableCell>
                  <TableCell className="text-right tabular-nums font-semibold">
                    {m.avg_latency_ms > 0 ? fmtMs(m.avg_latency_ms) : '—'}
                  </TableCell>
                  <TableCell className="text-right">
                    {m.success_rate != null ? (
                      <span className={`text-sm font-semibold tabular-nums ${
                        m.success_rate >= 90 ? 'text-status-success-fg'
                          : m.success_rate >= 70 ? 'text-status-warning-fg'
                          : 'text-status-error-fg'
                      }`}>
                        {fmtPct(m.success_rate)}
                      </span>
                    ) : '—'}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </DataTable>
        </CardContent>
      </Card>

      {chartData.length > 0 && (
        <Card className="xl:col-span-2">
          <CardHeader>
            <CardTitle className="text-base">{t('performance.modelLatency')}</CardTitle>
            <p className="text-xs text-muted-foreground">avg ms (top 10)</p>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={Math.max(160, chartData.length * 34)}>
              <BarChart data={chartData} layout="vertical" margin={{ left: 4, right: 24 }}>
                <XAxis type="number" tick={AXIS_TICK} axisLine={false} tickLine={false} tickFormatter={(v) => `${v}ms`} />
                <YAxis type="category" dataKey="name" width={130} tick={{ ...AXIS_TICK, fontSize: 10 }} axisLine={false} tickLine={false} />
                <Tooltip contentStyle={TOOLTIP_STYLE} labelStyle={TOOLTIP_LABEL_STYLE} itemStyle={TOOLTIP_ITEM_STYLE} cursor={CURSOR_FILL} formatter={(v) => [`${v}ms`, t('performance.avgLatency')] as [string, string]} />
                <Bar dataKey="latency" name={t('performance.avgLatency')} fill="var(--theme-status-info)" radius={[0, 4, 4, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
