import { cn } from '@/lib/utils'

/**
 * Shared inline progress bar used across capacity, usage, and breakdown views.
 *
 * Usage:
 *   <ProgressBar pct={72} colorClass="vds-bg-error" />          // Tailwind fill
 *   <ProgressBar pct={45} colorStyle={tokens.brand.primary} />     // inline style fill
 *   <ProgressBar pct={30} height="vds-h-2" className="vds-flex-1" />       // custom size/layout
 */
export function ProgressBar({
  pct,
  colorClass = 'vds-bg-primary',
  colorStyle,
  height = 'vds-h-1.5',
  trackClass = 'vds-bg-muted',
  className,
}: {
  pct: number
  colorClass?: string
  colorStyle?: string
  height?: string
  trackClass?: string
  className?: string
}) {
  const w = `${Math.min(Math.max(pct, 0), 100)}%`
  return (
    <div className={cn(height, 'vds-rounded-full vds-overflow-hidden', trackClass, className)}>
      <div
        className={cn('vds-h-full vds-rounded-full vds-transition-all', !colorStyle && colorClass)}
        style={{ width: w, ...(colorStyle ? { background: colorStyle } : {}) }}
      />
    </div>
  )
}
