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
  if (online === total) return 'vds-text-success'
  if (online > 0)       return 'vds-text-warning'
  return 'vds-text-error'
}

export function pendingValueCls(count: number): string {
  if (count === 0)  return 'vds-text-success'
  if (count < 10)   return 'vds-text-warning'
  return 'vds-text-error'
}

export function latencyColor(val: number | null | undefined, warnMs: number, errMs: number): string {
  if (val == null) return ''
  if (val >= errMs)  return 'vds-text-error'
  if (val >= warnMs) return 'vds-text-warning'
  return ''
}

export const THERMAL_ROW_CLS: Record<ThermalLevel, string> = {
  normal:   '',
  warning:  'vds-bg-warning/5 vds-border-l-2 vds-border-warning/60',
  critical: 'vds-bg-error/5 vds-border-l-2 vds-border-error/60',
  unknown:  '',
}

export const THERMAL_NAME_CLS: Record<ThermalLevel, string> = {
  normal:   '',
  warning:  'vds-text-warning',
  critical: 'vds-text-error',
  unknown:  '',
}

/* ─── sub-components ──────────────────────────────────────── */
export function StatSkeleton() {
  return (
    <Card aria-busy="true">
      <CardContent className="vds-p-5">
        <div className="vds-h-3 vds-w-24 vds-rounded vds-bg-muted vds-animate-pulse vds-mb-4" />
        <div className="vds-h-8 vds-w-16 vds-rounded vds-bg-muted vds-animate-pulse vds-mb-2" />
        <div className="vds-h-2 vds-w-20 vds-rounded vds-bg-muted vds-animate-pulse" />
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
    <div className="vds-flex vds-items-center vds-justify-between vds-py-2">
      <div className="vds-flex vds-items-center vds-gap-2 vds-text-sm vds-font-500">
        <Icon className="vds-h-4 vds-w-4" />
        <span>{label}</span>
        <span className="vds-text-dim vds-text-xs">({providers.length})</span>
      </div>
      <div className="vds-flex vds-items-center vds-gap-3 vds-text-xs">
        {online > 0 && (
          <span className="vds-flex vds-items-center vds-gap-1 vds-text-success">
            <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-inline-block" />
            {online}
          </span>
        )}
        {degraded > 0 && (
          <span className="vds-flex vds-items-center vds-gap-1 vds-text-warning">
            <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-warning vds-inline-block" />
            {degraded}
          </span>
        )}
        {offline > 0 && (
          <span className="vds-flex vds-items-center vds-gap-1 vds-text-dim">
            <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-neutral vds-inline-block" />
            {offline}
          </span>
        )}
        {providers.length === 0 && <span className="vds-text-dim">—</span>}
      </div>
    </div>
  )
})

export const ThermalLevelBadge = memo(function ThermalLevelBadge({ level, temp }: {
  level: ThermalLevel
  temp: number | null
}) {
  const { t } = useTranslation()
  if (level === 'unknown') return <span className="vds-text-2xs vds-text-dim">—</span>

  const cfg = {
    normal:   { cls: 'vds-text-success',  Icon: CheckCircle2,  key: 'overview.tempNormal' },
    warning:  { cls: 'vds-text-warning',  Icon: AlertTriangle, key: 'overview.tempWarning' },
    critical: { cls: 'vds-text-error',    Icon: XCircle,       key: 'overview.tempCritical' },
  }[level as Exclude<ThermalLevel, 'unknown'>]

  return (
    <span className={`vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 ${cfg.cls}`}>
      <cfg.Icon className="vds-h-3 vds-w-3" />
      <span>{t(cfg.key)}</span>
      {temp != null && <span className="vds-tabular-nums vds-opacity-70">({fmtTemp(temp)})</span>}
    </span>
  )
})

export const ConnectionDot = memo(function ConnectionDot({ connected }: { connected: boolean }) {
  const { t } = useTranslation()
  return connected ? (
    <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-success">
      <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-success vds-inline-block" />
      {t('overview.connected')}
    </span>
  ) : (
    <span className="vds-flex vds-items-center vds-gap-1 vds-text-2xs vds-font-500 vds-text-error">
      <span className="vds-h-1.5 vds-w-1.5 vds-rounded-full vds-bg-error vds-inline-block" />
      {t('overview.unreachable')}
    </span>
  )
})
