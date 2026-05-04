'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

export const Label = React.forwardRef<HTMLLabelElement, React.LabelHTMLAttributes<HTMLLabelElement>>(
  ({ className, ...props }, ref) => (
    <label
      ref={ref}
      className={cn(
        'vds-text-sm vds-font-500 vds-leading-tight vds-text-primary',
        'has-[+input:disabled]:vds-cursor-not-allowed has-[+input:disabled]:vds-opacity-70',
        className,
      )}
      {...props}
    />
  ),
)
Label.displayName = 'Label'
