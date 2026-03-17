import { cn } from '@/lib/utils'

/**
 * Shared inline progress bar used across capacity, usage, and breakdown views.
 *
 * Usage:
 *   <ProgressBar pct={72} colorClass="bg-status-error" />          // Tailwind fill
 *   <ProgressBar pct={45} colorStyle={tokens.brand.primary} />     // inline style fill
 *   <ProgressBar pct={30} height="h-2" className="flex-1" />       // custom size/layout
 */
export function ProgressBar({
  pct,
  colorClass = 'bg-primary',
  colorStyle,
  height = 'h-1.5',
  trackClass = 'bg-muted',
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
    <div className={cn(height, 'rounded-full overflow-hidden', trackClass, className)}>
      <div
        className={cn('h-full rounded-full transition-all', !colorStyle && colorClass)}
        style={{ width: w, ...(colorStyle ? { background: colorStyle } : {}) }}
      />
    </div>
  )
}
