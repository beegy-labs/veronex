import { clsx } from 'clsx'
import type { ReactNode } from 'react'

interface StatsCardProps {
  title: string
  value: string | number
  subtitle?: string
  icon?: ReactNode
  className?: string
}

export default function StatsCard({
  title,
  value,
  subtitle,
  icon,
  className,
}: StatsCardProps) {
  return (
    <div
      className={clsx(
        'rounded-xl border border-slate-800 bg-slate-900 p-5 flex flex-col gap-3',
        className,
      )}
    >
      <div className="flex items-center justify-between">
        <p className="text-sm font-medium text-slate-400">{title}</p>
        {icon && (
          <span className="text-slate-500">{icon}</span>
        )}
      </div>
      <p className="text-3xl font-bold text-slate-100 tabular-nums">
        {value.toLocaleString()}
      </p>
      {subtitle && (
        <p className="text-xs text-slate-500">{subtitle}</p>
      )}
    </div>
  )
}
