'use client'

import type { UsageBreakdown } from '@/lib/types'
import { Card, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { fmtCompact, fmtCost } from '@/lib/chart-theme'
import { calcPercentage } from '@/lib/utils'
import { useTranslation } from '@/i18n'
import { PROVIDER_BADGE, PROVIDER_COLORS } from '@/lib/constants'
import { tokens } from '@/lib/design-tokens'
import { ProgressBar } from '@/components/progress-bar'

export function ProviderBreakdownSection({ data }: { data: UsageBreakdown }) {
  const { t } = useTranslation()
  if (data.by_providers.length === 0) return (
    <div className="vds-py-12 vds-text-center vds-text-dim vds-text-sm">{t('usage.noData')}</div>
  )
  const total = data.by_providers.reduce((s, b) => s + b.request_count, 0)

  return (
    <div className="vds-grid vds-grid-cols-1 vds-sm:grid-cols-2 vds-gap-4">
      {data.by_providers.slice(0, 6).map((b) => {
        const pct = calcPercentage(b.request_count, total)
        const color = PROVIDER_COLORS[b.provider_type] ?? tokens.brand.primary
        const totalTok = b.prompt_tokens + b.completion_tokens
        return (
          <Card key={b.provider_type} className="vds-overflow-hidden">
            <CardContent className="vds-p-4 vds-space-y-3">
              <div className="vds-flex vds-items-center vds-justify-between">
                <Badge variant="outline" className={`vds-text-xs vds-font-mono vds-whitespace-nowrap ${PROVIDER_BADGE[b.provider_type] ?? ''}`}>
                  {b.provider_type}
                </Badge>
                <span className="vds-text-2xl vds-font-700 vds-tabular-nums">{fmtCompact(b.request_count)}</span>
              </div>
              <div>
                <div className="vds-flex vds-justify-between vds-text-xs vds-text-dim vds-mb-1">
                  <span>{t('usage.callShare')}</span>
                  <span className="vds-font-600 vds-tabular-nums" style={{ color }}>{pct}%</span>
                </div>
                <ProgressBar pct={pct} colorStyle={color} />
              </div>
              <div className="vds-grid vds-grid-cols-1 vds-sm:grid-cols-3 vds-gap-2 vds-text-xs">
                <div>
                  <p className="vds-text-dim">{t('usage.successCol')}</p>
                  <p className="vds-font-600 vds-tabular-nums vds-text-success">{b.success_rate}%</p>
                </div>
                <div>
                  <p className="vds-text-dim">{t('usage.tokensCol')}</p>
                  <p className="vds-font-600 vds-tabular-nums">{fmtCompact(totalTok)}</p>
                </div>
                <div>
                  <p className="vds-text-dim">{t('usage.errors')}</p>
                  <p className={`vds-font-600 vds-tabular-nums ${b.error_count > 0 ? 'vds-text-error' : 'vds-text-dim'}`}>
                    {fmtCompact(b.error_count)}
                  </p>
                </div>
              </div>
              {b.estimated_cost_usd != null && (
                <div className="vds-pt-2 vds-border-t-1 vds-border-subtle vds-text-xs vds-flex vds-justify-between vds-items-center">
                  <span className="vds-text-dim">{t('usage.estimatedCost')}</span>
                  <span className={`vds-font-600 vds-tabular-nums vds-font-mono ${b.estimated_cost_usd > 0 ? 'vds-text-primary' : 'vds-text-dim'}`}>
                    {b.estimated_cost_usd === 0 ? t('usage.free') : fmtCost(b.estimated_cost_usd)}
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
