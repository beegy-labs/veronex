'use client'

import { useMemo } from 'react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { DonutChart } from '@/components/donut-chart'
import { fmtCompact } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { useTranslation } from '@/i18n'
import { tokens } from '@/lib/design-tokens'
import { ProgressBar } from '@/components/progress-bar'

export function TokenDonut({ prompt, completion }: { prompt: number; completion: number }) {
  const { t } = useTranslation()
  const total = prompt + completion
  const data = useMemo(() => [
    { name: t('usage.promptTokens'), value: prompt,     pct: calcPercentage(prompt, total) },
    { name: t('usage.completionTokens'), value: completion, pct: calcPercentage(completion, total) },
  ], [prompt, completion, total, t])
  if (total === 0) return null
  return (
    <Card className="vds-h-full">
      <CardHeader>
        <CardTitle className="vds-text-base">{t('usage.tokenComposition')}</CardTitle>
        <p className="vds-text-xs vds-text-dim">{t('usage.tokenCompositionDesc')}</p>
      </CardHeader>
      <CardContent>
        <div className="vds-flex vds-items-center vds-gap-8">
          <DonutChart
            data={[
              { name: t('usage.promptTokens'),     value: prompt,     fill: tokens.brand.primary },
              { name: t('usage.completionTokens'), value: completion, fill: tokens.status.info },
            ]}
            size={140}
            innerRadius={38}
            outerRadius={60}
            formatter={fmtCompact}
          />
          <div className="vds-flex-1 vds-space-y-4">
            {data.map((d, i) => (
              <div key={d.name}>
                <div className="vds-flex vds-items-center vds-justify-between vds-mb-1">
                  <div className="vds-flex vds-items-center vds-gap-2">
                    <span className="vds-inline-block vds-h-2.5 vds-w-2.5 vds-rounded-full vds-flex-shrink-0"
                      style={{ background: i === 0 ? tokens.brand.primary : tokens.status.info }} />
                    <span className="vds-text-xs vds-font-700 vds-uppercase vds-tracking-widest vds-text-dim">{d.name}</span>
                  </div>
                  <span className="vds-text-sm vds-font-mono vds-font-700">{d.pct}%</span>
                </div>
                <ProgressBar pct={d.pct} colorStyle={i === 0 ? tokens.brand.primary : tokens.status.info} />
                <p className="vds-text-xs vds-text-dim vds-mt-1">{t('usage.nTokens', { n: fmtCompact(d.value) })}</p>
              </div>
            ))}
            <p className="vds-text-xs vds-text-dim vds-pt-1 vds-border-t-1 vds-border-subtle">
              {t('usage.totalNTokens', { n: fmtCompact(total) })}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
