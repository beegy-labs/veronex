import { cn } from '@/lib/utils'

/**
 * Shared section heading label used across dashboard, usage, and modal views.
 *
 * Usage:
 *   <SectionLabel>Tokens per hour</SectionLabel>
 *   <SectionLabel as="h2">Infrastructure</SectionLabel>
 *   <SectionLabel className="vds-mb-4">Model usage</SectionLabel>
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
    <Tag className={cn('vds-text-2xs vds-font-black vds-uppercase vds-tracking-[0.3em] vds-text-dim vds-mb-3', className)}>
      {children}
    </Tag>
  )
}
