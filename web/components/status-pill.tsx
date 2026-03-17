'use client'

/**
 * Count pill used in page headers to show resource status breakdowns.
 * Pass `className` with the full colour variant (bg-*, border-*, text-*).
 *
 * Default (no className): muted neutral pill — used for total registered count.
 */
export function StatusPill({
  icon,
  count,
  label,
  className,
}: {
  icon?: React.ReactNode
  count: number
  label: string
  className?: string
}) {
  return (
    <div className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium ${className ?? 'bg-muted/60 border border-border text-muted-foreground'}`}>
      {icon}
      <span className="tabular-nums">{count}</span>
      <span>{label}</span>
    </div>
  )
}
