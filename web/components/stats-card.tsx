import type { ReactNode } from 'react'
import { Card, CardContent } from '@/components/ui/card'

interface StatsCardProps {
  title: string
  value: string | number
  subtitle?: string
  /** Overrides `subtitle` when set — allows rich ReactNode content. */
  subtitleNode?: ReactNode
  icon?: ReactNode
  className?: string
  /** Optional Tailwind color class applied to the value text. */
  valueClassName?: string
}

export default function StatsCard({ title, value, subtitle, subtitleNode, icon, className, valueClassName }: StatsCardProps) {
  return (
    <Card className={className}>
      <CardContent className="vds-p-5">
        <div className="vds-flex vds-items-center vds-justify-between vds-mb-2">
          <p className="vds-text-sm vds-font-500 vds-text-dim">{title}</p>
          {icon && <span className="vds-text-dim">{icon}</span>}
        </div>
        <p className={`vds-text-3xl vds-font-700 vds-tabular-nums ${valueClassName ?? ''}`}>{String(value)}</p>
        {subtitleNode
          ? <div className="vds-mt-1 vds-text-xs">{subtitleNode}</div>
          : subtitle && <p className="vds-mt-1 vds-text-xs vds-text-dim">{subtitle}</p>
        }
      </CardContent>
    </Card>
  )
}
