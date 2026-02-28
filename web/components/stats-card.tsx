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
}

export default function StatsCard({ title, value, subtitle, subtitleNode, icon, className }: StatsCardProps) {
  return (
    <Card className={className}>
      <CardContent className="p-5">
        <div className="flex items-center justify-between mb-2">
          <p className="text-sm font-medium text-muted-foreground">{title}</p>
          {icon && <span className="text-muted-foreground">{icon}</span>}
        </div>
        <p className="text-3xl font-bold tabular-nums">{String(value)}</p>
        {subtitleNode
          ? <div className="mt-1 text-xs">{subtitleNode}</div>
          : subtitle && <p className="mt-1 text-xs text-muted-foreground">{subtitle}</p>
        }
      </CardContent>
    </Card>
  )
}
