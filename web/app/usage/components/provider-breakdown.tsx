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
    <div className="py-12 text-center text-muted-foreground text-sm">{t('usage.noData')}</div>
  )
  const total = data.by_providers.reduce((s, b) => s + b.request_count, 0)

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
      {data.by_providers.slice(0, 6).map((b) => {
        const pct = calcPercentage(b.request_count, total)
        const color = PROVIDER_COLORS[b.provider_type] ?? tokens.brand.primary
        const totalTok = b.prompt_tokens + b.completion_tokens
        return (
          <Card key={b.provider_type} className="overflow-hidden">
            <CardContent className="p-4 space-y-3">
              <div className="flex items-center justify-between">
                <Badge variant="outline" className={`text-xs font-mono ${PROVIDER_BADGE[b.provider_type] ?? ''}`}>
                  {b.provider_type}
                </Badge>
                <span className="text-2xl font-bold tabular-nums">{fmtCompact(b.request_count)}</span>
              </div>
              <div>
                <div className="flex justify-between text-xs text-muted-foreground mb-1">
                  <span>{t('usage.callShare')}</span>
                  <span className="font-semibold tabular-nums" style={{ color }}>{pct}%</span>
                </div>
                <ProgressBar pct={pct} colorStyle={color} />
              </div>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-2 text-xs">
                <div>
                  <p className="text-muted-foreground">{t('usage.successCol')}</p>
                  <p className="font-semibold tabular-nums text-status-success-fg">{b.success_rate}%</p>
                </div>
                <div>
                  <p className="text-muted-foreground">{t('usage.tokensCol')}</p>
                  <p className="font-semibold tabular-nums">{fmtCompact(totalTok)}</p>
                </div>
                <div>
                  <p className="text-muted-foreground">{t('usage.errors')}</p>
                  <p className={`font-semibold tabular-nums ${b.error_count > 0 ? 'text-status-error-fg' : 'text-muted-foreground'}`}>
                    {fmtCompact(b.error_count)}
                  </p>
                </div>
              </div>
              {b.estimated_cost_usd != null && (
                <div className="pt-2 border-t border-border text-xs flex justify-between items-center">
                  <span className="text-muted-foreground">{t('usage.estimatedCost')}</span>
                  <span className={`font-semibold tabular-nums font-mono ${b.estimated_cost_usd > 0 ? 'text-foreground' : 'text-muted-foreground'}`}>
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
