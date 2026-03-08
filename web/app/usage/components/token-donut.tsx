'use client'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { DonutChart } from '@/components/donut-chart'
import { fmtCompact } from '@/lib/chart-theme'
import { useTranslation } from '@/i18n'

export function TokenDonut({ prompt, completion }: { prompt: number; completion: number }) {
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
