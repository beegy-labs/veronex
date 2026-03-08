'use client'

import type { AnalyticsStats } from '@/lib/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { DonutChart } from '@/components/donut-chart'
import { useTranslation } from '@/i18n'
import { FINISH_COLORS, FINISH_BG } from '@/lib/constants'

export function FinishReasonsCard({ data }: { data: AnalyticsStats }) {
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
