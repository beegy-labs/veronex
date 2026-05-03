'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

interface SeparatorProps extends React.HTMLAttributes<HTMLDivElement> {
  orientation?: 'horizontal' | 'vertical'
  decorative?: boolean
}

export const Separator = React.forwardRef<HTMLDivElement, SeparatorProps>(
  ({ className, orientation = 'horizontal', decorative = true, ...props }, ref) => (
    <div
      ref={ref}
      role={decorative ? 'none' : 'separator'}
      aria-orientation={decorative ? undefined : orientation}
      className={cn(
        'vds-flex-shrink-0 vds-bg-border-default',
        orientation === 'horizontal' ? 'vds-h-px vds-w-full' : 'vds-h-full vds-w-px',
        className,
      )}
      {...props}
    />
  ),
)
Separator.displayName = 'Separator'
