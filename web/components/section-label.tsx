import { cn } from '@/lib/utils'

/**
 * Shared section heading label used across dashboard, usage, and modal views.
 *
 * Usage:
 *   <SectionLabel>Tokens per hour</SectionLabel>
 *   <SectionLabel as="h2">Infrastructure</SectionLabel>
 *   <SectionLabel className="mb-4">Model usage</SectionLabel>
 */
export function SectionLabel({
  children,
  className,
  as: Tag = 'p',
}: {
  children: React.ReactNode
  className?: string
  as?: 'p' | 'h2' | 'h3' | 'span'
}) {
  return (
    <Tag className={cn('text-[11px] font-black uppercase tracking-[0.3em] text-muted-foreground mb-3', className)}>
      {children}
    </Tag>
  )
}
