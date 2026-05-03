'use client'

import { useMemo } from 'react'
import type { AnalyticsStats } from '@/lib/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { DonutChart } from '@/components/donut-chart'
import { useTranslation } from '@/i18n'
import { FINISH_COLORS, FINISH_BG } from '@/lib/constants'
import { tokens } from '@/lib/design-tokens'
import { calcPercentage } from '@/lib/utils'

const FINISH_REASON_LABEL_KEY: Record<string, string> = {
  stop:      'usage.finishStop',
  length:    'usage.finishLength',
  error:     'usage.finishError',
  cancelled: 'usage.finishCancelled',
}

export function FinishReasonsCard({ data }: { data: AnalyticsStats }) {
  const { t } = useTranslation()
  const donutData = useMemo(() => {
    const total = data.finish_reasons.reduce((s, r) => s + r.count, 0)
    return data.finish_reasons.map((r) => ({
      name: r.reason,
      value: r.count,
      pct: calcPercentage(r.count, total),
    }))
  }, [data.finish_reasons])
  if (donutData.length === 0) return null

  return (
    <Card className="vds-h-full">
      <CardHeader>
        <CardTitle className="vds-text-base">{t('usage.finishReasonTitle')}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="vds-flex vds-items-center vds-gap-6">
          <DonutChart
            data={donutData.map((d) => ({
              name: t(FINISH_REASON_LABEL_KEY[d.name] ?? 'usage.finishStop'),
              value: d.value,
              fill: FINISH_COLORS[d.name] ?? tokens.text.faint,
            }))}
            size={120}
            innerRadius={30}
            outerRadius={50}
            formatter={(v) => String(v)}
          />
          <div className="vds-flex-1 vds-space-y-2">
            {donutData.map((d) => (
              <div key={d.name} className="vds-flex vds-items-center vds-justify-between vds-gap-2">
                <div className="vds-flex vds-items-center vds-gap-2">
                  <span className="vds-h-2 vds-w-2 vds-rounded-full vds-flex-shrink-0"
                    style={{ background: FINISH_COLORS[d.name] ?? tokens.text.faint }} />
                  <span className={`vds-text-xs vds-font-500 vds-px-1.5 vds-py-0.5 vds-rounded vds-border-1 ${FINISH_BG[d.name] ?? 'vds-bg-muted vds-text-dim vds-border-default'}`}>
                    {t(FINISH_REASON_LABEL_KEY[d.name] ?? 'usage.finishStop')}
                  </span>
                </div>
                <div className="vds-text-right">
                  <span className="vds-text-sm vds-font-mono vds-tabular-nums vds-font-700">{d.value}</span>
                  <span className="vds-text-xs vds-text-dim vds-ml-1">({d.pct}%)</span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
