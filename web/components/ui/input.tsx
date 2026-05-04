'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

export const Input = React.forwardRef<HTMLInputElement, React.ComponentProps<'input'>>(
  ({ className, type, ...props }, ref) => (
    <input
      ref={ref}
      type={type}
      className={cn(
        'vds-flex vds-h-9 vds-w-full vds-rounded-md vds-border-1 vds-border-default',
        'vds-bg-card vds-px-3 vds-py-1 vds-text-sm vds-text-primary',
        'vds-placeholder:text-faint',
        'vds-transition-colors',
        'vds-disabled:cursor-not-allowed vds-disabled:opacity-50',
        className,
      )}
      {...props}
    />
  ),
)
Input.displayName = 'Input'
