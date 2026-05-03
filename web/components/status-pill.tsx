'use client'

/**
 * Count pill used in page headers to show resource status breakdowns.
 * Pass `className` with the full colour variant (bg-*, border-*, text-*).
 *
 * Default (no className): muted neutral pill — used for total registered count.
 * When `count` is omitted, only the label is shown (e.g. pagination info).
 */
export function StatusPill({
  icon,
  count,
  label,
  className,
}: {
  icon?: React.ReactNode
  count?: number
  label: string
  className?: string
}) {
  return (
    <div className={`vds-flex vds-items-center vds-gap-1.5 vds-px-2.5 vds-py-1 vds-rounded-full vds-text-xs vds-font-500 vds-whitespace-nowrap ${className ?? 'vds-bg-muted/60 vds-border-1 vds-border-subtle vds-text-dim'}`}>
      {icon}
      {count !== undefined && <span className="vds-tabular-nums">{count}</span>}
      <span>{label}</span>
    </div>
  )
}
