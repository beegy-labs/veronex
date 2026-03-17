'use client'

import { memo, useMemo } from 'react'
import { fmtTemp } from '@/lib/chart-theme'
import { useTranslation } from '@/i18n'
import { Card, CardContent } from '@/components/ui/card'
import { CheckCircle2, XCircle, AlertTriangle } from 'lucide-react'
import type { Provider } from '@/lib/types'

/* ─── pure color helpers ──────────────────────────────────── */
export type ThermalLevel = 'normal' | 'warning' | 'critical' | 'unknown'

export function providerValueCls(online: number, total: number): string {
  if (total === 0) return ''
  if (online === total) return 'text-status-success-fg'
  if (online > 0)       return 'text-status-warning-fg'
  return 'text-status-error-fg'
}

export function pendingValueCls(count: number): string {
  if (count === 0)  return 'text-status-success-fg'
  if (count < 10)   return 'text-status-warning-fg'
  return 'text-status-error-fg'
}

export function latencyColor(val: number | null | undefined, warnMs: number, errMs: number): string {
  if (val == null) return ''
  if (val >= errMs)  return 'text-status-error-fg'
  if (val >= warnMs) return 'text-status-warning-fg'
  return ''
}

export const THERMAL_ROW_CLS: Record<ThermalLevel, string> = {
  normal:   '',
  warning:  'bg-status-warning/5 border-l-2 border-status-warning/60',
  critical: 'bg-status-error/5 border-l-2 border-status-error/60',
  unknown:  '',
}

export const THERMAL_NAME_CLS: Record<ThermalLevel, string> = {
  normal:   '',
  warning:  'text-status-warning-fg',
  critical: 'text-status-error-fg',
  unknown:  '',
}

/* ─── sub-components ──────────────────────────────────────── */
export function StatSkeleton() {
  return (
    <Card aria-busy="true">
      <CardContent className="p-5">
        <div className="h-3 w-24 rounded bg-muted animate-pulse mb-4" />
        <div className="h-8 w-16 rounded bg-muted animate-pulse mb-2" />
        <div className="h-2 w-20 rounded bg-muted animate-pulse" />
      </CardContent>
    </Card>
  )
}

export const ProviderRow = memo(function ProviderRow({
  Icon, label, providers,
}: {
  Icon: React.ComponentType<{ className?: string }>
  label: string
  providers: Provider[]
}) {
  const { online, degraded, offline } = useMemo(() => ({
    online:   providers.filter(b => b.status === 'online').length,
    degraded: providers.filter(b => b.status === 'degraded').length,
    offline:  providers.filter(b => b.status === 'offline').length,
  }), [providers])

  return (
    <div className="flex items-center justify-between py-2">
      <div className="flex items-center gap-2 text-sm font-medium">
        <Icon className="h-4 w-4" />
        <span>{label}</span>
        <span className="text-muted-foreground text-xs">({providers.length})</span>
      </div>
      <div className="flex items-center gap-3 text-xs">
        {online > 0 && (
          <span className="flex items-center gap-1 text-status-success-fg">
            <span className="h-1.5 w-1.5 rounded-full bg-status-success inline-block" />
            {online}
          </span>
        )}
        {degraded > 0 && (
          <span className="flex items-center gap-1 text-status-warning-fg">
            <span className="h-1.5 w-1.5 rounded-full bg-status-warning inline-block" />
            {degraded}
          </span>
        )}
        {offline > 0 && (
          <span className="flex items-center gap-1 text-muted-foreground">
            <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground inline-block" />
            {offline}
          </span>
        )}
        {providers.length === 0 && <span className="text-muted-foreground">—</span>}
      </div>
    </div>
  )
})

export const ThermalLevelBadge = memo(function ThermalLevelBadge({ level, temp }: {
  level: ThermalLevel
  temp: number | null
}) {
  const { t } = useTranslation()
  if (level === 'unknown') return <span className="text-[11px] text-muted-foreground">—</span>

  const cfg = {
    normal:   { cls: 'text-status-success-fg',  Icon: CheckCircle2,  key: 'overview.tempNormal' },
    warning:  { cls: 'text-status-warning-fg',  Icon: AlertTriangle, key: 'overview.tempWarning' },
    critical: { cls: 'text-status-error-fg',    Icon: XCircle,       key: 'overview.tempCritical' },
  }[level as Exclude<ThermalLevel, 'unknown'>]

  return (
    <span className={`flex items-center gap-1 text-[11px] font-medium ${cfg.cls}`}>
      <cfg.Icon className="h-3 w-3" />
      <span>{t(cfg.key)}</span>
      {temp != null && <span className="tabular-nums opacity-70">({fmtTemp(temp)})</span>}
    </span>
  )
})

export const ConnectionDot = memo(function ConnectionDot({ connected }: { connected: boolean }) {
  const { t } = useTranslation()
  return connected ? (
    <span className="flex items-center gap-1 text-[11px] font-medium text-status-success-fg">
      <span className="h-1.5 w-1.5 rounded-full bg-status-success inline-block" />
      {t('overview.connected')}
    </span>
  ) : (
    <span className="flex items-center gap-1 text-[11px] font-medium text-status-error-fg">
      <span className="h-1.5 w-1.5 rounded-full bg-status-error inline-block" />
      {t('overview.unreachable')}
    </span>
  )
})
